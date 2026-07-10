//! capture/tc4.rs — the TC4/Arduino serial `DeviceSource` (CONTRACTS.md §7.2).
//!
//! READ-ONLY invariant (hard, enforced by construction): this driver NEVER
//! writes OT1/OT2/IO3/PID/DCFAN/heater commands — the only bytes it can put on
//! the wire come from [`ReadOnlyCommand`], a closed enum of the four
//! configuration/poll verbs. No other write path exists; do not add one.
//!
//! Protocol provenance (clean-room; see oss-staging/docs/protocols/
//! tc4-aartisanq.md for the full provenance table): the aArtisanQ protocol
//! documentation (`commands.txt`) and firmware configuration headers,
//! MLG Properties LLC / Jim Gallt — greencardigan/TC4-shield, BSD-3-Clause —
//! plus our own serial captures. No GPL implementation (Artisan's or any
//! other) was read, ported, or paraphrased for this driver.
//!
//! Protocol summary: line-based ASCII at 115200 8N1, `\n`-terminated.
//! `CHAN;1200` maps physical inputs to logical channels; `UNITS;F`/`UNITS;C`
//! picks the reporting unit; `READ` polls one CSV sample:
//! `ambient,ch1,ch2[,ch3,ch4][,heater,fan[,SV]]` — field count varies by
//! firmware build, so frames are parsed positionally and tolerantly.
//! `#`-prefixed lines are acks/boot chatter; blank/garbled lines are skipped;
//! partial reads are buffered to the newline. Arduino-based rigs DTR-reset
//! when the port opens: allow ~2 s of boot settle before the first command.
//!
//! Failure matrix (§7.2): 3 consecutive READ failures → `status: disconnected`
//! → reconnect loop with 2 s..30 s backoff, re-init + probe on success →
//! `status: reconnected`, seq continues. `stop()` signals and JOINS the read
//! thread (< 500 ms; every wait in the loop is interruptible and port reads
//! use a short chunk timeout).

use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serialport::SerialPort;

use crate::error::LoggerError;
use crate::model::{
    SampleDto, SourceInfo, SourceKind, SourcePinDto, SourceStatusKind, TempUnitDto,
};

use super::serial::{open_port, LineOutcome, LineReader};
use super::{DeviceSource, SampleSink, SourceEmit};

/// How many consecutive READ failures flip the driver to `disconnected`.
const FAILURE_THRESHOLD: u32 = 3;

// ---------------------------------------------------------------------------
// The closed, read-only command set
// ---------------------------------------------------------------------------

/// Every byte this driver can transmit. A closed set of configuration/poll
/// verbs — none of them actuate hardware. This enum IS the read-only
/// invariant: there is no API for writing anything else to the port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadOnlyCommand {
    /// `CHAN;1200` — TC1 → logical 1, TC2 → logical 2, TC3/TC4 off.
    Chan,
    /// `UNITS;F` / `UNITS;C` — reporting unit for all values incl. ambient.
    Units(TempUnitDto),
    /// `READ` — poll one CSV sample.
    Read,
}

impl ReadOnlyCommand {
    fn line(self) -> &'static str {
        match self {
            ReadOnlyCommand::Chan => "CHAN;1200",
            ReadOnlyCommand::Units(TempUnitDto::F) => "UNITS;F",
            ReadOnlyCommand::Units(TempUnitDto::C) => "UNITS;C",
            ReadOnlyCommand::Read => "READ",
        }
    }
}

pub(crate) fn send_command(
    port: &mut dyn SerialPort,
    cmd: ReadOnlyCommand,
) -> Result<(), std::io::Error> {
    port.write_all(cmd.line().as_bytes())?;
    port.write_all(b"\n")?;
    port.flush()
}

// ---------------------------------------------------------------------------
// Timing (accelerated by tests; defaults are the real-hardware values)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Tc4Timing {
    /// Wait after opening the port before the first command (DTR boot reset).
    pub boot_settle: Duration,
    /// READ poll cadence.
    pub poll_interval: Duration,
    /// Timeout for one `port.read()` call — keeps stop() joins fast.
    pub chunk_timeout: Duration,
    /// Longest wait for the data line answering a READ.
    pub response_deadline: Duration,
    /// Longest wait for the `#` ack after CHAN/UNITS (missing acks tolerated).
    pub ack_deadline: Duration,
    /// Reconnect backoff bounds (doubling).
    pub backoff_min: Duration,
    pub backoff_max: Duration,
}

impl Default for Tc4Timing {
    fn default() -> Self {
        Self {
            boot_settle: Duration::from_millis(2000),
            poll_interval: Duration::from_millis(1000),
            chunk_timeout: Duration::from_millis(200),
            response_deadline: Duration::from_millis(900),
            ack_deadline: Duration::from_millis(600),
            backoff_min: Duration::from_secs(2),
            backoff_max: Duration::from_secs(30),
        }
    }
}

// ---------------------------------------------------------------------------
// Frame parsing (positional, tolerant — field count varies by firmware)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Tc4Frame {
    pub ambient: f64,
    /// Logical channels in 1-based TC4 order (index 0 = logical channel 1).
    pub channels: Vec<f64>,
    pub heater: Option<f64>,
    pub fan: Option<f64>,
}

/// All-numeric CSV fields of a candidate data line; `None` for `#` acks,
/// blank lines, or anything non-numeric (tolerated and skipped).
pub(crate) fn numeric_csv_fields(line: &str) -> Option<Vec<f64>> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let mut fields = Vec::new();
    for raw in line.split(',') {
        fields.push(raw.trim().parse::<f64>().ok()?);
    }
    Some(fields)
}

/// Parse one READ data line: field 1 is ambient, then up to four channel
/// fields; if at least two more remain they are heater/fan duty (a trailing
/// SV field on some builds is ignored). Never indexes a fixed layout.
pub(crate) fn parse_data_line(line: &str) -> Option<Tc4Frame> {
    let fields = numeric_csv_fields(line)?;
    if fields.len() < 2 {
        return None;
    }
    let ambient = fields[0];
    let ch_end = fields.len().min(5);
    let channels = fields[1..ch_end].to_vec();
    let extras = &fields[ch_end.max(5).min(fields.len())..];
    let (heater, fan) = if extras.len() >= 2 {
        (Some(extras[0]), Some(extras[1]))
    } else {
        (None, None)
    };
    Some(Tc4Frame {
        ambient,
        channels,
        heater,
        fan,
    })
}

pub(crate) fn to_fahrenheit(value: f64, unit: TempUnitDto) -> f64 {
    match unit {
        TempUnitDto::F => value,
        TempUnitDto::C => value * 9.0 / 5.0 + 32.0,
    }
}

/// Map a frame through the pin (1-based btChannel/etChannel, °C→°F when the
/// pin unit is C). `None` when the BT channel is missing OR non-finite — an
/// unusable frame that counts as a read failure.
///
/// Finiteness matters: `"nan"`/`"inf"` parse as valid `f64`s (Arduino
/// `Print::printFloat` emits them for open/failed thermocouples), but a NaN
/// BT binds as SQL NULL and fails the `NOT NULL` samples constraint — every
/// persist would fail while the driver kept reporting healthy. Rejecting the
/// BT per-field (not the whole frame in `numeric_csv_fields`) keeps rigs with
/// an unused channel slot printing `nan` fully usable, and routes an open BT
/// probe into the documented 3-strike disconnected → reconnect flow. The
/// optional fields are sanitized to `None` so neither NaN-as-NULL coincidence
/// nor a silently-stored `Inf` ever reaches the DB.
fn sample_from_frame(
    frame: &Tc4Frame,
    pin: &SourcePinDto,
    seq: u64,
    session_sec: f64,
) -> Option<SampleDto> {
    let channel = |n: u32| -> Option<f64> {
        (n >= 1)
            .then(|| frame.channels.get((n - 1) as usize).copied())
            .flatten()
    };
    let finite = |v: &f64| v.is_finite();
    let bt = channel(pin.bt_channel).filter(finite)?;
    let et_f = pin
        .et_channel
        .and_then(channel)
        .filter(finite)
        .map(|v| to_fahrenheit(v, pin.unit));
    Some(SampleDto {
        seq,
        session_sec,
        bt_f: to_fahrenheit(bt, pin.unit),
        et_f,
        ambient_f: frame
            .ambient
            .is_finite()
            .then(|| to_fahrenheit(frame.ambient, pin.unit)),
        heater: frame.heater.filter(finite),
        fan: frame.fan.filter(finite),
        drum: None,
    })
}

// ---------------------------------------------------------------------------
// Interruptible waits
// ---------------------------------------------------------------------------

/// Sleep until `deadline`, returning `false` immediately if stop is signalled.
fn wait_until(deadline: Instant, stop_rx: &mpsc::Receiver<()>) -> bool {
    loop {
        let now = Instant::now();
        if now >= deadline {
            return true;
        }
        match stop_rx.recv_timeout(deadline - now) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => return false,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Session init / polling (shared with the preview + sniffer in serial.rs)
// ---------------------------------------------------------------------------

pub(crate) enum PollOutcome {
    Frame(Tc4Frame),
    Failed,
    Stopped,
}

/// Boot-settle, drain the boot chatter, then CHAN + UNITS. Missing acks are
/// tolerated (ack text varies by firmware build); write failures are not.
/// Returns `None` on stop, `Some(ok)` otherwise.
pub(crate) fn init_device(
    port: &mut dyn SerialPort,
    reader: &mut LineReader,
    unit: TempUnitDto,
    timing: &Tc4Timing,
    stop_rx: &mpsc::Receiver<()>,
) -> Option<bool> {
    if !wait_until(Instant::now() + timing.boot_settle, stop_rx) {
        return None;
    }
    let _ = port.clear(serialport::ClearBuffer::Input);
    reader.reset();
    for cmd in [ReadOnlyCommand::Chan, ReadOnlyCommand::Units(unit)] {
        if send_command(port, cmd).is_err() {
            return Some(false);
        }
        // Consume the ack (or give up quietly at the deadline).
        let deadline = Instant::now() + timing.ack_deadline;
        loop {
            match reader.read_line(port, deadline, stop_rx) {
                LineOutcome::Line(line) if line.trim_start().starts_with('#') => break,
                LineOutcome::Line(_) => continue, // stale data line — keep looking
                LineOutcome::Timeout => break,    // ack never came; tolerated
                LineOutcome::Stopped => return None,
                LineOutcome::Eof | LineOutcome::Failed => return Some(false),
            }
        }
    }
    Some(true)
}

/// One READ poll: send the command, wait for a parseable data line.
pub(crate) fn poll_once(
    port: &mut dyn SerialPort,
    reader: &mut LineReader,
    timing: &Tc4Timing,
    stop_rx: &mpsc::Receiver<()>,
) -> PollOutcome {
    if send_command(port, ReadOnlyCommand::Read).is_err() {
        return PollOutcome::Failed;
    }
    let deadline = Instant::now() + timing.response_deadline;
    loop {
        match reader.read_line(port, deadline, stop_rx) {
            LineOutcome::Line(line) => {
                if let Some(frame) = parse_data_line(&line) {
                    if !frame.channels.is_empty() {
                        return PollOutcome::Frame(frame);
                    }
                }
                // `#` ack, blank, or garbled line — skip and keep reading.
            }
            LineOutcome::Timeout | LineOutcome::Eof | LineOutcome::Failed => {
                return PollOutcome::Failed
            }
            LineOutcome::Stopped => return PollOutcome::Stopped,
        }
    }
}

// ---------------------------------------------------------------------------
// Tc4Source
// ---------------------------------------------------------------------------

pub struct Tc4Source {
    port_name: String,
    /// Channel roles + baud + unit (§5 SourcePin shape).
    pin: SourcePinDto,
    /// First seq this source will emit (1 for a fresh session,
    /// `last_persisted + 1` when resuming).
    start_seq: u64,
    /// Session clock offset: 0 for a fresh session; the last persisted
    /// session_sec when resuming (the clock continues from there + elapsed).
    start_session_sec: f64,
    timing: Tc4Timing,
    worker: Option<(mpsc::SyncSender<()>, JoinHandle<()>)>,
}

impl Tc4Source {
    pub fn new(
        port_name: String,
        pin: SourcePinDto,
        start_seq: u64,
        start_session_sec: f64,
    ) -> Self {
        Self {
            port_name,
            pin,
            start_seq: start_seq.max(1),
            start_session_sec: start_session_sec.max(0.0),
            timing: Tc4Timing::default(),
            worker: None,
        }
    }

    pub fn with_timing(mut self, timing: Tc4Timing) -> Self {
        self.timing = timing;
        self
    }
}

impl DeviceSource for Tc4Source {
    fn descriptor(&self) -> SourceInfo {
        SourceInfo {
            id: format!("tc4:{}", self.port_name),
            label: format!("TC4 — {}", self.port_name),
            kind: SourceKind::Device,
        }
    }

    fn start(&mut self, sink: SampleSink) -> Result<(), LoggerError> {
        if self.worker.is_some() {
            return Err(LoggerError::io("tc4 source already started"));
        }
        // Open synchronously so start_session fails fast with port_busy /
        // port_error; boot settle + init happen on the read thread.
        let port = open_port(&self.port_name, self.pin.baud, self.timing.chunk_timeout)?;
        let (stop_tx, stop_rx) = mpsc::sync_channel::<()>(1);
        let ctx = CaptureCtx {
            port_name: self.port_name.clone(),
            pin: self.pin.clone(),
            timing: self.timing.clone(),
            start_seq: self.start_seq,
            start_session_sec: self.start_session_sec,
        };
        let thread_name = format!(
            "tc4-{}",
            self.port_name.rsplit('/').next().unwrap_or(&self.port_name)
        );
        let handle = std::thread::Builder::new()
            .name(thread_name)
            .spawn(move || run_capture(port, ctx, &stop_rx, &sink))
            .map_err(|e| LoggerError::io(format!("failed to spawn tc4 thread: {e}")))?;
        self.worker = Some((stop_tx, handle));
        Ok(())
    }

    fn stop(&mut self) {
        if let Some((stop_tx, handle)) = self.worker.take() {
            let _ = stop_tx.send(());
            let _ = handle.join();
        }
    }
}

impl Drop for Tc4Source {
    fn drop(&mut self) {
        self.stop();
    }
}

struct CaptureCtx {
    port_name: String,
    pin: SourcePinDto,
    timing: Tc4Timing,
    start_seq: u64,
    start_session_sec: f64,
}

fn run_capture(
    mut port: Box<dyn SerialPort>,
    ctx: CaptureCtx,
    stop_rx: &mpsc::Receiver<()>,
    sink: &SampleSink,
) {
    let anchor = Instant::now();
    let session_sec = |ctx: &CaptureCtx| ctx.start_session_sec + anchor.elapsed().as_secs_f64();
    let mut reader = LineReader::new();
    let mut seq = ctx.start_seq;
    let mut failures: u32 = 0;

    match init_device(&mut *port, &mut reader, ctx.pin.unit, &ctx.timing, stop_rx) {
        None => return,
        Some(true) => {}
        // A dead write path right at start: fall through and let the READ
        // failures drive the normal disconnected → reconnect flow.
        Some(false) => failures = FAILURE_THRESHOLD.saturating_sub(1),
    }

    let mut next_poll = Instant::now();
    loop {
        if !wait_until(next_poll, stop_rx) {
            return;
        }
        next_poll += ctx.timing.poll_interval;
        if next_poll < Instant::now() {
            next_poll = Instant::now(); // catch up after a slow response
        }

        match poll_once(&mut *port, &mut reader, &ctx.timing, stop_rx) {
            PollOutcome::Stopped => return,
            PollOutcome::Frame(frame) => {
                match sample_from_frame(&frame, &ctx.pin, seq, session_sec(&ctx)) {
                    Some(sample) => {
                        failures = 0;
                        sink(SourceEmit::Sample(sample));
                        seq += 1;
                    }
                    None => failures += 1, // frame with a missing/non-finite BT channel
                }
            }
            PollOutcome::Failed => failures += 1,
        }

        if failures >= FAILURE_THRESHOLD {
            sink(SourceEmit::Status {
                kind: SourceStatusKind::Disconnected,
                message: Some(format!("lost contact with {}; reconnecting", ctx.port_name)),
                at_session_sec: session_sec(&ctx),
            });
            drop(port);
            match reconnect(&ctx, &mut reader, stop_rx) {
                Some(reconnected) => {
                    port = reconnected;
                    failures = 0;
                    sink(SourceEmit::Status {
                        kind: SourceStatusKind::Reconnected,
                        message: None,
                        at_session_sec: session_sec(&ctx),
                    });
                    next_poll = Instant::now();
                }
                None => return, // stopped while reconnecting
            }
        }
    }
}

/// Reopen with doubling backoff (2 s..30 s by default). An attempt only
/// counts as success after re-init AND a probe READ answers — so a port that
/// opens but stays mute keeps backing off instead of flapping
/// reconnected/disconnected. Returns `None` when stopped.
fn reconnect(
    ctx: &CaptureCtx,
    reader: &mut LineReader,
    stop_rx: &mpsc::Receiver<()>,
) -> Option<Box<dyn SerialPort>> {
    let mut backoff = ctx.timing.backoff_min;
    loop {
        if !wait_until(Instant::now() + backoff, stop_rx) {
            return None;
        }
        backoff = (backoff * 2).min(ctx.timing.backoff_max);
        let Ok(mut port) = open_port(&ctx.port_name, ctx.pin.baud, ctx.timing.chunk_timeout) else {
            continue;
        };
        match init_device(&mut *port, reader, ctx.pin.unit, &ctx.timing, stop_rx) {
            None => return None,
            Some(false) => continue,
            Some(true) => {}
        }
        match poll_once(&mut *port, reader, &ctx.timing, stop_rx) {
            PollOutcome::Stopped => return None,
            // The probe must yield a USABLE sample (BT present and finite),
            // not just any frame — a rig whose BT probe is still open keeps
            // backing off instead of flapping disconnected/reconnected.
            PollOutcome::Frame(frame) if sample_from_frame(&frame, &ctx.pin, 0, 0.0).is_some() => {
                return Some(port); // probe frame not emitted
            }
            PollOutcome::Frame(_) | PollOutcome::Failed => continue,
        }
    }
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

/// In-process TC4 emulator on a pty — the no-hardware test rig (the
/// `tc4_emulator` example is the standalone contributor-facing twin).
/// Unix-only: Windows CI uses the replay simulator instead.
#[cfg(all(test, unix))]
pub(crate) mod emu {
    use std::io::{Read, Write};
    use std::os::fd::AsRawFd;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use crate::model::TempUnitDto;

    /// Piecewise-linear BT curve in °F over wall-clock seconds; held flat
    /// past the last knot.
    #[derive(Clone)]
    pub struct EmuConfig {
        pub curve: Vec<(f64, f64)>,
        /// 1-based slot BT is reported on (of 4 channel fields).
        pub bt_slot: usize,
        /// 1-based slot ET (= BT + 40°F) is reported on; 0 = omit.
        pub et_slot: usize,
        /// Append heater/fan duty fields (the 7-field aArtisanQ shape).
        pub power_fields: bool,
    }

    impl Default for EmuConfig {
        fn default() -> Self {
            Self {
                curve: vec![(0.0, 390.0)],
                bt_slot: 1,
                et_slot: 2,
                power_fields: true,
            }
        }
    }

    pub struct Emulator {
        pub slave_path: String,
        /// While false, READ goes unanswered (the "unplugged" lever).
        pub responsive: Arc<AtomicBool>,
        /// Every command line the driver ever sent — the read-only audit log.
        pub commands: Arc<Mutex<Vec<String>>>,
        stop: Arc<AtomicBool>,
        handle: Option<std::thread::JoinHandle<()>>,
    }

    impl Emulator {
        pub fn spawn(config: EmuConfig) -> Self {
            let pty = nix::pty::openpty(None, None).expect("openpty");
            // Raw mode: no echo, no ONLCR — the pty must move bytes verbatim.
            let mut termios = nix::sys::termios::tcgetattr(&pty.slave).expect("tcgetattr");
            nix::sys::termios::cfmakeraw(&mut termios);
            nix::sys::termios::tcsetattr(&pty.slave, nix::sys::termios::SetArg::TCSANOW, &termios)
                .expect("tcsetattr");
            let slave_path = nix::unistd::ttyname(&pty.slave)
                .expect("ttyname")
                .to_string_lossy()
                .into_owned();
            // Drop our slave fd: the driver must be able to open/close/reopen
            // the path exclusively (serialport sets TIOCEXCL); the pty stays
            // alive through the master.
            drop(pty.slave);

            // Non-blocking master so the emulator loop can poll its stop flag.
            let flags =
                nix::fcntl::fcntl(&pty.master, nix::fcntl::FcntlArg::F_GETFL).expect("F_GETFL");
            let mut oflags = nix::fcntl::OFlag::from_bits_truncate(flags);
            oflags.insert(nix::fcntl::OFlag::O_NONBLOCK);
            nix::fcntl::fcntl(&pty.master, nix::fcntl::FcntlArg::F_SETFL(oflags)).expect("F_SETFL");

            let responsive = Arc::new(AtomicBool::new(true));
            let commands: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
            let stop = Arc::new(AtomicBool::new(false));
            let handle = {
                let responsive = Arc::clone(&responsive);
                let commands = Arc::clone(&commands);
                let stop = Arc::clone(&stop);
                let master = std::fs::File::from(pty.master);
                std::thread::Builder::new()
                    .name("tc4-emu".into())
                    .spawn(move || run(master, config, &responsive, &commands, &stop))
                    .expect("spawn emulator")
            };
            Self {
                slave_path,
                responsive,
                commands,
                stop,
                handle: Some(handle),
            }
        }

        pub fn set_responsive(&self, on: bool) {
            self.responsive.store(on, Ordering::SeqCst);
        }

        pub fn sent_commands(&self) -> Vec<String> {
            self.commands.lock().unwrap().clone()
        }
    }

    impl Drop for Emulator {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    fn curve_at(curve: &[(f64, f64)], t: f64) -> f64 {
        match curve.iter().position(|&(kt, _)| kt >= t) {
            Some(0) => curve[0].1,
            Some(i) => {
                let (t0, v0) = curve[i - 1];
                let (t1, v1) = curve[i];
                if t1 <= t0 {
                    v1
                } else {
                    v0 + (v1 - v0) * (t - t0) / (t1 - t0)
                }
            }
            None => curve.last().map(|&(_, v)| v).unwrap_or(0.0),
        }
    }

    fn run(
        mut master: std::fs::File,
        config: EmuConfig,
        responsive: &AtomicBool,
        commands: &Mutex<Vec<String>>,
        stop: &AtomicBool,
    ) {
        let _ = master.as_raw_fd(); // keep the fd import meaningful on all cfgs
        let anchor = Instant::now();
        let mut unit = TempUnitDto::F;
        let mut buf: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 256];
        loop {
            if stop.load(Ordering::SeqCst) {
                return;
            }
            match master.read(&mut chunk) {
                Ok(0) => {
                    // No slave open right now (driver closed for reconnect).
                    std::thread::sleep(Duration::from_millis(2));
                }
                Ok(n) => {
                    buf.extend_from_slice(&chunk[..n]);
                    while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                        let raw: Vec<u8> = buf.drain(..=pos).collect();
                        let line = String::from_utf8_lossy(&raw).trim().to_string();
                        if line.is_empty() {
                            continue;
                        }
                        commands.lock().unwrap().push(line.clone());
                        if !responsive.load(Ordering::SeqCst) {
                            continue;
                        }
                        let reply = respond(&line, &config, &mut unit, anchor);
                        let mut bytes = reply.as_bytes();
                        while !bytes.is_empty() {
                            match master.write(bytes) {
                                Ok(w) => bytes = &bytes[w..],
                                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                    std::thread::sleep(Duration::from_millis(1));
                                }
                                Err(_) => break,
                            }
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(_) => std::thread::sleep(Duration::from_millis(2)),
            }
        }
    }

    fn respond(line: &str, config: &EmuConfig, unit: &mut TempUnitDto, anchor: Instant) -> String {
        let verb = line.split(';').next().unwrap_or("");
        match verb {
            "CHAN" => "# Active channels set\n".to_string(),
            "UNITS" => {
                *unit = if line.ends_with('C') {
                    TempUnitDto::C
                } else {
                    TempUnitDto::F
                };
                "#OK\n".to_string()
            }
            "FILT" => "#OK\n".to_string(),
            "READ" => {
                let t = anchor.elapsed().as_secs_f64();
                let bt_f = curve_at(&config.curve, t);
                let conv = |f: f64| match *unit {
                    TempUnitDto::F => f,
                    TempUnitDto::C => (f - 32.0) * 5.0 / 9.0,
                };
                let mut slots = [0.0f64; 4];
                if (1..=4).contains(&config.bt_slot) {
                    slots[config.bt_slot - 1] = conv(bt_f);
                }
                if (1..=4).contains(&config.et_slot) {
                    slots[config.et_slot - 1] = conv(bt_f + 40.0);
                }
                let mut out = format!(
                    "{:.2},{:.2},{:.2},{:.2},{:.2}",
                    conv(72.0),
                    slots[0],
                    slots[1],
                    slots[2],
                    slots[3]
                );
                if config.power_fields {
                    out.push_str(",45.00,60.00");
                }
                out.push('\n');
                out
            }
            // Control verbs (OT1/OT2/IO3/PID/DCFAN/…) would land here: the
            // Logger must never send one, and the audit log proves it.
            _ => "#REJECTED\n".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-and-run verification that the `nix` dev-dependency's `term`
    /// feature exposes `openpty` — the seam the TC4 emulator example
    /// (`examples/tc4_emulator.rs`) builds on.
    #[test]
    fn nix_openpty_is_available_for_the_emulator_example() {
        let pty = nix::pty::openpty(
            None::<&nix::pty::Winsize>,
            None::<&nix::sys::termios::Termios>,
        )
        .expect("openpty must be available");
        drop(pty);
    }

    #[test]
    fn parses_all_documented_field_count_variants() {
        // 3 fields: ambient,ch1,ch2 (aArtisan, 2 channels)
        let f = parse_data_line("72.50,367.25,341.00").unwrap();
        assert_eq!(f.ambient, 72.5);
        assert_eq!(f.channels, vec![367.25, 341.0]);
        assert_eq!((f.heater, f.fan), (None, None));

        // 5 fields: 4 channels
        let f = parse_data_line("72.50,367.25,341.00,0.00,0.00").unwrap();
        assert_eq!(f.channels.len(), 4);
        assert_eq!((f.heater, f.fan), (None, None));

        // 7 fields: + heater/fan duty
        let f = parse_data_line("72.50,367.25,341.00,0.00,0.00,45.00,60.00").unwrap();
        assert_eq!((f.heater, f.fan), (Some(45.0), Some(60.0)));

        // 8 fields: + SV (ignored)
        let f = parse_data_line("72.5,367.25,341.0,0,0,45,60,410").unwrap();
        assert_eq!((f.heater, f.fan), (Some(45.0), Some(60.0)));

        // 6 fields: 4 channels + one ambiguous extra — extra ignored
        let f = parse_data_line("72.5,367.25,341.0,0,0,45").unwrap();
        assert_eq!(f.channels.len(), 4);
        assert_eq!((f.heater, f.fan), (None, None));

        // Whitespace + \r tolerated
        let f = parse_data_line(" 72.5 , 367.25 , 341.0 \r").unwrap();
        assert_eq!(f.channels, vec![367.25, 341.0]);

        // Acks, blanks, garbage → None
        assert!(parse_data_line("#OK").is_none());
        assert!(parse_data_line("# Active channels set to 1200").is_none());
        assert!(parse_data_line("").is_none());
        assert!(parse_data_line("hello,world").is_none());
        assert!(parse_data_line("72.5,367.25,NaN-ish?").is_none());
        assert!(
            parse_data_line("72.5").is_none(),
            "ambient alone is not a sample"
        );
    }

    #[test]
    fn sample_mapping_respects_pin_channels_and_units() {
        let frame = Tc4Frame {
            ambient: 22.0,
            channels: vec![180.0, 200.0, 0.0, 0.0],
            heater: Some(45.0),
            fan: Some(60.0),
        };
        // BT on ch2, ET on ch1, °C source
        let pin = SourcePinDto {
            source_id: "tc4:x".into(),
            baud: 115_200,
            bt_channel: 2,
            et_channel: Some(1),
            unit: TempUnitDto::C,
        };
        let s = sample_from_frame(&frame, &pin, 7, 12.5).unwrap();
        assert_eq!(s.seq, 7);
        assert_eq!(s.session_sec, 12.5);
        assert!((s.bt_f - (200.0 * 9.0 / 5.0 + 32.0)).abs() < 1e-9);
        assert!((s.et_f.unwrap() - (180.0 * 9.0 / 5.0 + 32.0)).abs() < 1e-9);
        assert!((s.ambient_f.unwrap() - 71.6).abs() < 1e-9);
        assert_eq!((s.heater, s.fan), (Some(45.0), Some(60.0)));

        // Missing BT channel → unusable frame
        let pin_bad = SourcePinDto {
            bt_channel: 4,
            ..pin.clone()
        };
        let short = Tc4Frame {
            ambient: 22.0,
            channels: vec![180.0, 200.0],
            heater: None,
            fan: None,
        };
        assert!(sample_from_frame(&short, &pin_bad, 1, 0.0).is_none());

        // ET channel absent from the frame → et_f = None, sample still usable
        let pin_et4 = SourcePinDto {
            bt_channel: 1,
            et_channel: Some(4),
            unit: TempUnitDto::F,
            ..pin
        };
        let s = sample_from_frame(&short, &pin_et4, 1, 0.0).unwrap();
        assert_eq!(s.bt_f, 180.0);
        assert_eq!(s.et_f, None);
    }

    #[test]
    fn non_finite_fields_never_reach_a_sample() {
        // "nan"/"inf" parse as valid f64s (Rust FromStr), so the frame itself
        // parses — real firmware prints them for open thermocouples…
        let f = parse_data_line("72.5,nan,341.0").unwrap();
        assert!(f.channels[0].is_nan());
        assert_eq!(f.channels[1], 341.0);

        let pin = |bt: u32, et: Option<u32>| SourcePinDto {
            source_id: "tc4:x".into(),
            baud: 115_200,
            bt_channel: bt,
            et_channel: et,
            unit: TempUnitDto::F,
        };

        // …but a non-finite BT is an unusable frame: it must count as a read
        // failure (3-strike disconnect), never bind NaN → SQL NULL → a failed
        // NOT NULL persist behind a healthy driver status.
        assert!(sample_from_frame(&f, &pin(1, None), 1, 0.0).is_none());
        let inf = parse_data_line("72.5,inf,341.0").unwrap();
        assert!(
            sample_from_frame(&inf, &pin(1, None), 1, 0.0).is_none(),
            "Inf BT rejected too"
        );

        // nan in an UNUSED slot must not invalidate a healthy frame…
        let s = sample_from_frame(&f, &pin(2, None), 1, 0.0).expect("healthy BT on ch2");
        assert_eq!(s.bt_f, 341.0);
        // …and a non-finite ET degrades to None instead of killing the frame.
        let s = sample_from_frame(&f, &pin(2, Some(1)), 1, 0.0).unwrap();
        assert_eq!((s.bt_f, s.et_f), (341.0, None));

        // Non-finite optional fields are sanitized to explicit None (no
        // NaN-as-NULL coincidence, no silently-stored Inf).
        let dirty = Tc4Frame {
            ambient: f64::NAN,
            channels: vec![341.0],
            heater: Some(f64::INFINITY),
            fan: Some(f64::NAN),
        };
        let s = sample_from_frame(&dirty, &pin(1, None), 1, 0.0).unwrap();
        assert_eq!(s.ambient_f, None);
        assert_eq!((s.heater, s.fan), (None, None));
    }

    #[test]
    fn read_only_command_set_is_closed_and_exact() {
        assert_eq!(ReadOnlyCommand::Chan.line(), "CHAN;1200");
        assert_eq!(ReadOnlyCommand::Units(TempUnitDto::F).line(), "UNITS;F");
        assert_eq!(ReadOnlyCommand::Units(TempUnitDto::C).line(), "UNITS;C");
        assert_eq!(ReadOnlyCommand::Read.line(), "READ");
    }

    #[cfg(unix)]
    mod pty {
        use super::super::emu::{EmuConfig, Emulator};
        use super::*;
        use crate::capture::{SampleSink, SourceEmit};
        use std::sync::{Arc, Mutex};

        pub(crate) fn fast_timing() -> Tc4Timing {
            Tc4Timing {
                boot_settle: Duration::from_millis(30),
                poll_interval: Duration::from_millis(20),
                chunk_timeout: Duration::from_millis(10),
                response_deadline: Duration::from_millis(80),
                ack_deadline: Duration::from_millis(40),
                backoff_min: Duration::from_millis(40),
                backoff_max: Duration::from_millis(160),
            }
        }

        fn collector() -> (Arc<Mutex<Vec<SourceEmit>>>, SampleSink) {
            let collected: Arc<Mutex<Vec<SourceEmit>>> = Arc::new(Mutex::new(Vec::new()));
            let sink: SampleSink = {
                let collected = Arc::clone(&collected);
                Arc::new(move |emit| collected.lock().unwrap().push(emit))
            };
            (collected, sink)
        }

        fn wait_for<F: Fn(&[SourceEmit]) -> bool>(
            collected: &Arc<Mutex<Vec<SourceEmit>>>,
            what: &str,
            pred: F,
        ) {
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                if pred(&collected.lock().unwrap()) {
                    return;
                }
                assert!(Instant::now() < deadline, "timed out waiting for {what}");
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        fn sample_count(events: &[SourceEmit]) -> usize {
            events
                .iter()
                .filter(|e| matches!(e, SourceEmit::Sample(_)))
                .count()
        }

        fn has_status(events: &[SourceEmit], kind: SourceStatusKind) -> bool {
            events
                .iter()
                .any(|e| matches!(e, SourceEmit::Status { kind: k, .. } if *k == kind))
        }

        #[test]
        fn missing_port_fails_fast_with_port_error() {
            let pin = SourcePinDto {
                source_id: "tc4:/dev/definitely-not-a-port".into(),
                baud: 115_200,
                bt_channel: 1,
                et_channel: None,
                unit: TempUnitDto::F,
            };
            let mut source = Tc4Source::new("/dev/definitely-not-a-port".into(), pin, 1, 0.0)
                .with_timing(fast_timing());
            let (_, sink) = collector();
            let err = source.start(sink).unwrap_err();
            assert_eq!(err.code, crate::error::ErrorCode::PortError);
        }

        #[test]
        fn captures_maps_units_survives_unplug_and_stays_read_only() {
            // BT on ch2, ET on ch1, device reporting °C — the full mapping.
            let emulator = Emulator::spawn(EmuConfig {
                curve: vec![(0.0, 390.0)],
                bt_slot: 2,
                et_slot: 1,
                power_fields: true,
            });
            let pin = SourcePinDto {
                source_id: format!("tc4:{}", emulator.slave_path),
                baud: 115_200,
                bt_channel: 2,
                et_channel: Some(1),
                unit: TempUnitDto::C,
            };
            let (collected, sink) = collector();
            let mut source =
                Tc4Source::new(emulator.slave_path.clone(), pin, 5, 0.0).with_timing(fast_timing());
            source.start(sink).unwrap();

            // Phase 1: samples flow, correctly mapped and unit-converted.
            wait_for(&collected, "5 samples", |e| sample_count(e) >= 5);
            {
                let events = collected.lock().unwrap();
                let samples: Vec<&SampleDto> = events
                    .iter()
                    .filter_map(|e| match e {
                        SourceEmit::Sample(s) => Some(s),
                        _ => None,
                    })
                    .collect();
                assert_eq!(
                    samples[0].seq, 5,
                    "seq starts at the engine-provided start_seq"
                );
                for (i, s) in samples.iter().enumerate() {
                    assert_eq!(s.seq, 5 + i as u64, "seq must be contiguous");
                    assert!(
                        (s.bt_f - 390.0).abs() < 0.2,
                        "bt °C→°F round trip, got {}",
                        s.bt_f
                    );
                    assert!((s.et_f.unwrap() - 430.0).abs() < 0.2, "et = bt + 40°F");
                    assert!((s.ambient_f.unwrap() - 72.0).abs() < 0.2);
                    assert_eq!((s.heater, s.fan), (Some(45.0), Some(60.0)));
                }
                for pair in samples.windows(2) {
                    assert!(
                        pair[1].session_sec > pair[0].session_sec,
                        "session clock must be monotonic"
                    );
                }
            }

            // Phase 2: the device goes mute → disconnected.
            emulator.set_responsive(false);
            wait_for(&collected, "disconnected status", |e| {
                has_status(e, SourceStatusKind::Disconnected)
            });

            // Phase 3: it comes back → reconnected, seq continues gap-free.
            let before = sample_count(&collected.lock().unwrap());
            emulator.set_responsive(true);
            wait_for(&collected, "reconnected status", |e| {
                has_status(e, SourceStatusKind::Reconnected)
            });
            wait_for(&collected, "post-reconnect samples", |e| {
                sample_count(e) >= before + 3
            });

            // stop() must join quickly (< 500 ms contract).
            let t0 = Instant::now();
            source.stop();
            assert!(
                t0.elapsed() < Duration::from_millis(500),
                "stop took {:?}",
                t0.elapsed()
            );

            {
                let events = collected.lock().unwrap();
                let seqs: Vec<u64> = events
                    .iter()
                    .filter_map(|e| match e {
                        SourceEmit::Sample(s) => Some(s.seq),
                        _ => None,
                    })
                    .collect();
                assert!(
                    seqs.windows(2).all(|w| w[1] == w[0] + 1),
                    "no seq gaps across the outage"
                );
                // Ordering: disconnected strictly before reconnected.
                let disc = events
                    .iter()
                    .position(|e| {
                        matches!(
                            e,
                            SourceEmit::Status {
                                kind: SourceStatusKind::Disconnected,
                                ..
                            }
                        )
                    })
                    .unwrap();
                let reco = events
                    .iter()
                    .position(|e| {
                        matches!(
                            e,
                            SourceEmit::Status {
                                kind: SourceStatusKind::Reconnected,
                                ..
                            }
                        )
                    })
                    .unwrap();
                assert!(disc < reco);
            }

            // READ-ONLY invariant, audited: the driver only ever transmitted
            // the closed command set — never a control verb.
            let sent = emulator.sent_commands();
            assert!(!sent.is_empty());
            for cmd in &sent {
                let verb = cmd.split(';').next().unwrap();
                assert!(
                    matches!(verb, "CHAN" | "UNITS" | "FILT" | "READ"),
                    "non-read-only command on the wire: {cmd}"
                );
            }
            assert!(sent.iter().any(|c| c == "CHAN;1200"));
            assert!(sent.iter().any(|c| c == "UNITS;C"));
        }

        /// Regression (nan-frame rejection): a BT thermocouple reading open
        /// ("nan" on the wire) must drive the documented 3-strike
        /// disconnected → reconnect flow — with ZERO samples emitted while
        /// BT is non-finite — and recover cleanly once readings return.
        /// Pre-fix, nan frames mapped as healthy samples (failures reset,
        /// status stuck `connected`) and disconnected never fired.
        #[test]
        fn nan_bt_frames_count_as_failures_and_drive_disconnect_then_recovery() {
            // NaN BT until t = 0.6 s of wall time, then a healthy 390°F.
            // (Duplicate knot: curve_at returns the later value at t > 0.6.)
            let emulator = Emulator::spawn(EmuConfig {
                curve: vec![(0.0, f64::NAN), (0.6, f64::NAN), (0.6, 390.0)],
                bt_slot: 1,
                et_slot: 2,
                power_fields: true,
            });
            let pin = SourcePinDto {
                source_id: format!("tc4:{}", emulator.slave_path),
                baud: 115_200,
                bt_channel: 1,
                et_channel: Some(2),
                unit: TempUnitDto::F,
            };
            let (collected, sink) = collector();
            let mut source =
                Tc4Source::new(emulator.slave_path.clone(), pin, 1, 0.0).with_timing(fast_timing());
            source.start(sink).unwrap();

            wait_for(&collected, "disconnected on nan BT", |e| {
                has_status(e, SourceStatusKind::Disconnected)
            });
            wait_for(&collected, "reconnected once BT is finite", |e| {
                has_status(e, SourceStatusKind::Reconnected)
            });
            wait_for(&collected, "post-recovery samples", |e| {
                sample_count(e) >= 3
            });
            source.stop();

            let events = collected.lock().unwrap();
            let disc = events
                .iter()
                .position(|e| {
                    matches!(
                        e,
                        SourceEmit::Status {
                            kind: SourceStatusKind::Disconnected,
                            ..
                        }
                    )
                })
                .unwrap();
            assert!(
                events[..disc]
                    .iter()
                    .all(|e| !matches!(e, SourceEmit::Sample(_))),
                "no sample may be emitted while BT reads nan"
            );
            for e in events.iter() {
                if let SourceEmit::Sample(s) = e {
                    assert!(
                        s.bt_f.is_finite(),
                        "non-finite BT leaked into a sample: {s:?}"
                    );
                    assert!(
                        (s.bt_f - 390.0).abs() < 0.2,
                        "recovered BT wrong: {}",
                        s.bt_f
                    );
                }
            }
        }

        #[test]
        fn resume_offsets_the_session_clock() {
            let emulator = Emulator::spawn(EmuConfig::default());
            let pin = SourcePinDto {
                source_id: format!("tc4:{}", emulator.slave_path),
                baud: 115_200,
                bt_channel: 1,
                et_channel: Some(2),
                unit: TempUnitDto::F,
            };
            let (collected, sink) = collector();
            // Resuming at seq 43 with 84.0 s already on the session clock.
            let mut source = Tc4Source::new(emulator.slave_path.clone(), pin, 43, 84.0)
                .with_timing(fast_timing());
            source.start(sink).unwrap();
            wait_for(&collected, "3 resumed samples", |e| sample_count(e) >= 3);
            source.stop();
            let events = collected.lock().unwrap();
            let first = events
                .iter()
                .find_map(|e| match e {
                    SourceEmit::Sample(s) => Some(*s),
                    _ => None,
                })
                .unwrap();
            assert_eq!(first.seq, 43);
            assert!(
                first.session_sec > 84.0 && first.session_sec < 90.0,
                "clock continues from last_session_sec + elapsed, got {}",
                first.session_sec
            );
        }
    }
}
