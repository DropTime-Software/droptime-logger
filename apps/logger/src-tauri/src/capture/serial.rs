//! capture/serial.rs — serial-port enumeration, sniffing and live preview
//! (CONTRACTS.md §7.2).
//!
//! Protocol provenance: the aArtisanQ protocol documentation (`commands.txt`),
//! MLG Properties LLC / Jim Gallt — greencardigan/TC4-shield, BSD-3-Clause —
//! plus our own captures; see oss-staging/docs/protocols/tc4-aartisanq.md.
//! Clean-room: no GPL implementation was consulted.
//!
//! Everything here is READ-ONLY toward hardware: the only bytes ever sent are
//! the closed [`tc4::ReadOnlyCommand`] set (the sniffer sends at most ONE
//! `READ` probe).

use std::sync::mpsc;
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serialport::SerialPort;

use crate::error::LoggerError;
use crate::model::{
    PreviewEvent, SerialChip, SerialPortDto, SniffPortArgs, SniffResultDto, SniffVerdict,
    StartPortPreviewArgs, TempUnitDto,
};
use crate::store::now_ms;

use super::tc4::{
    init_device, numeric_csv_fields, poll_once, send_command, to_fahrenheit, PollOutcome,
    ReadOnlyCommand, Tc4Timing,
};

/// Where preview frames go — the IPC `Channel<PreviewEvent>` in production.
pub type PreviewEmitter = Box<dyn Fn(PreviewEvent) + Send + Sync>;

pub const DEFAULT_BAUD: u32 = 115_200;

// ---------------------------------------------------------------------------
// Port open + error mapping (shared with the TC4 driver)
// ---------------------------------------------------------------------------

/// Open `port_name` at 8N1/no-flow-control with a short per-read timeout.
/// Errors map to the §7 codes: busy/permission → `port_busy` (with the
/// "another app may hold the port" hint), missing/other → `port_error`.
///
/// Pseudo-terminals (the no-hardware emulator rig, `tc4_emulator`) reject the
/// baud-rate ioctl with `ENOTTY` — macOS' `IOSSIOSPEED` in particular only
/// works on real serial hardware. `serialport` documents `baud_rate == 0` as
/// the pty escape hatch (skips the ioctl, the flush, and DTR), and bytes over
/// a pty move at pipe speed regardless, so on that specific rejection we
/// retry once at 0 baud. Real devices never take this path.
pub(crate) fn open_port(
    port_name: &str,
    baud: u32,
    chunk_timeout: Duration,
) -> Result<Box<dyn SerialPort>, LoggerError> {
    raw_open(port_name, baud, chunk_timeout)
        .or_else(|err| {
            if is_pty_baud_rejection(&err) {
                raw_open(port_name, 0, chunk_timeout)
            } else {
                Err(err)
            }
        })
        .map_err(|err| map_open_error(port_name, &err))
}

fn raw_open(
    port_name: &str,
    baud: u32,
    chunk_timeout: Duration,
) -> Result<Box<dyn SerialPort>, serialport::Error> {
    serialport::new(port_name, baud)
        .data_bits(serialport::DataBits::Eight)
        .parity(serialport::Parity::None)
        .stop_bits(serialport::StopBits::One)
        .flow_control(serialport::FlowControl::None)
        .timeout(chunk_timeout)
        .open()
}

/// `ENOTTY` from the baud-setting ioctl: "Not a typewriter" (macOS/BSD) or
/// "Inappropriate ioctl for device" (Linux).
fn is_pty_baud_rejection(err: &serialport::Error) -> bool {
    let text = err.to_string().to_lowercase();
    text.contains("not a typewriter") || text.contains("inappropriate ioctl")
}

fn map_open_error(port_name: &str, err: &serialport::Error) -> LoggerError {
    let text = err.to_string().to_lowercase();
    let busy = matches!(
        err.kind,
        serialport::ErrorKind::Io(std::io::ErrorKind::PermissionDenied)
    ) || text.contains("busy")
        || text.contains("denied")
        || text.contains("in use")
        || text.contains("locked");
    if busy {
        return LoggerError::port_busy(format!(
            "{port_name} is busy or access was denied — another app (like Artisan) may be \
             holding the port. Close it and try again."
        ));
    }
    let missing = matches!(err.kind, serialport::ErrorKind::NoDevice)
        || matches!(
            err.kind,
            serialport::ErrorKind::Io(std::io::ErrorKind::NotFound)
        )
        || text.contains("no such file")
        || text.contains("not found");
    if missing {
        return LoggerError::port_error(format!(
            "{port_name} was not found — is the device plugged in?"
        ));
    }
    LoggerError::port_error(format!("could not open {port_name}: {err}"))
}

// ---------------------------------------------------------------------------
// Line reader: newline framing over partial reads
// ---------------------------------------------------------------------------

pub(crate) enum LineOutcome {
    /// One complete line, `\r`/`\n` stripped.
    Line(String),
    /// Deadline passed without a complete line (partial bytes stay buffered).
    Timeout,
    /// The port reported end-of-stream.
    Eof,
    /// Stop was signalled.
    Stopped,
    /// A non-timeout I/O error.
    Failed,
}

pub(crate) struct LineReader {
    buf: Vec<u8>,
}

impl LineReader {
    pub fn new() -> Self {
        Self {
            buf: Vec::with_capacity(512),
        }
    }

    /// Forget buffered partial input (used after boot-settle drains).
    pub fn reset(&mut self) {
        self.buf.clear();
    }

    fn take_line(&mut self) -> Option<String> {
        let pos = self.buf.iter().position(|&b| b == b'\n')?;
        let raw: Vec<u8> = self.buf.drain(..=pos).collect();
        let mut line = String::from_utf8_lossy(&raw).into_owned();
        while line.ends_with('\n') || line.ends_with('\r') {
            line.pop();
        }
        Some(line)
    }

    /// Read one newline-terminated line, waiting until `deadline`. Individual
    /// port reads use the port's own short timeout so `stop` stays responsive.
    pub fn read_line(
        &mut self,
        port: &mut dyn SerialPort,
        deadline: Instant,
        stop_rx: &mpsc::Receiver<()>,
    ) -> LineOutcome {
        let mut chunk = [0u8; 256];
        loop {
            if let Some(line) = self.take_line() {
                return LineOutcome::Line(line);
            }
            if matches!(
                stop_rx.try_recv(),
                Ok(()) | Err(mpsc::TryRecvError::Disconnected)
            ) {
                return LineOutcome::Stopped;
            }
            if Instant::now() >= deadline {
                return LineOutcome::Timeout;
            }
            match port.read(&mut chunk) {
                Ok(0) => return LineOutcome::Eof,
                Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::TimedOut
                            | std::io::ErrorKind::WouldBlock
                            | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(_) => return LineOutcome::Failed,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// list_serial_ports (§7.2)
// ---------------------------------------------------------------------------

/// VID → USB-serial bridge chip family. Identifies the bridge, not the
/// protocol — used only to rank likely TC4 rigs; the sniffer confirms by
/// frame shape.
fn chip_for_vid(vid: u16) -> SerialChip {
    match vid {
        0x0403 => SerialChip::Ftdi,
        0x10C4 => SerialChip::Cp210x,
        0x1A86 => SerialChip::Ch340,
        _ => SerialChip::Other,
    }
}

pub fn list_ports() -> Result<Vec<SerialPortDto>, LoggerError> {
    let ports = serialport::available_ports()
        .map_err(|e| LoggerError::port_error(format!("could not enumerate serial ports: {e}")))?;
    Ok(ports
        .into_iter()
        .map(|p| match p.port_type {
            serialport::SerialPortType::UsbPort(usb) => {
                let chip = chip_for_vid(usb.vid);
                SerialPortDto {
                    port_name: p.port_name,
                    vid: Some(usb.vid),
                    pid: Some(usb.pid),
                    manufacturer: usb.manufacturer,
                    product: usb.product,
                    serial_number: usb.serial_number,
                    chip: Some(chip),
                    likely: chip != SerialChip::Other,
                }
            }
            _ => SerialPortDto {
                port_name: p.port_name,
                vid: None,
                pid: None,
                manufacturer: None,
                product: None,
                serial_number: None,
                chip: None,
                likely: false,
            },
        })
        .collect())
}

// ---------------------------------------------------------------------------
// sniff_port (§7.2)
// ---------------------------------------------------------------------------

const SNIFF_FRAME_CAP: usize = 10;
/// A TC4 data frame for classification purposes: ≥3 numeric CSV fields
/// (ambient + at least two channels).
const SNIFF_MIN_FIELDS: usize = 3;

pub fn sniff(args: &SniffPortArgs) -> Result<SniffResultDto, LoggerError> {
    // Sniff is synchronous and never stopped — but the sender must stay ALIVE
    // for the duration (binding it to `_` would drop it immediately and every
    // read would see a disconnected channel as a stop signal).
    let (_stop_tx, stop_rx) = mpsc::sync_channel::<()>(1);
    sniff_with_stop(args, &stop_rx)
}

fn sniff_with_stop(
    args: &SniffPortArgs,
    stop_rx: &mpsc::Receiver<()>,
) -> Result<SniffResultDto, LoggerError> {
    let baud = args.baud.unwrap_or(DEFAULT_BAUD);
    let passive_window = Duration::from_millis(args.duration_ms.unwrap_or(3000));
    let mut port = open_port(&args.port_name, baud, Duration::from_millis(100))?;
    let mut reader = LineReader::new();

    let mut raw_frames: Vec<String> = Vec::new();
    let mut total_lines = 0usize;
    let mut data_frames = 0usize;
    let mut post_probe_frames = 0usize;
    let mut channels: Option<Vec<u32>> = None;

    let collect = |reader: &mut LineReader,
                   port: &mut dyn SerialPort,
                   window: Duration,
                   counter: &mut usize,
                   raw_frames: &mut Vec<String>,
                   channels: &mut Option<Vec<u32>>|
     -> usize {
        let deadline = Instant::now() + window;
        let mut data = 0usize;
        loop {
            match reader.read_line(port, deadline, stop_rx) {
                LineOutcome::Line(line) => {
                    *counter += 1;
                    if raw_frames.len() < SNIFF_FRAME_CAP {
                        raw_frames.push(line.clone());
                    }
                    if let Some(fields) = numeric_csv_fields(&line) {
                        if fields.len() >= SNIFF_MIN_FIELDS {
                            data += 1;
                            let seen: Vec<u32> = fields[1..fields.len().min(5)]
                                .iter()
                                .enumerate()
                                .filter(|(_, v)| **v != 0.0)
                                .map(|(i, _)| i as u32 + 1)
                                .collect();
                            if channels.as_ref().is_none_or(|c| seen.len() > c.len()) {
                                *channels = Some(seen);
                            }
                        }
                    }
                }
                LineOutcome::Timeout => return data,
                LineOutcome::Stopped | LineOutcome::Eof | LineOutcome::Failed => return data,
            }
        }
    };

    // Passive listen first: boot chatter (the open DTR-resets Arduinos) and
    // free-running logging-mode rigs both show up here without us sending a
    // single byte.
    data_frames += collect(
        &mut reader,
        &mut *port,
        passive_window,
        &mut total_lines,
        &mut raw_frames,
        &mut channels,
    );

    // Quiet (or nearly so)? ONE read-only READ probe, then ~1s more listening.
    let mut probed = false;
    if data_frames < 2 {
        probed = true;
        if send_command(&mut *port, ReadOnlyCommand::Read).is_ok() {
            post_probe_frames = collect(
                &mut reader,
                &mut *port,
                Duration::from_millis(1000),
                &mut total_lines,
                &mut raw_frames,
                &mut channels,
            );
            data_frames += post_probe_frames;
        }
    }

    let verdict = if data_frames >= 2 || post_probe_frames >= 1 {
        SniffVerdict::Tc4
    } else if total_lines > 0 {
        SniffVerdict::Unknown
    } else {
        SniffVerdict::Silent
    };

    let diagnostic = diagnostic_block(
        &args.port_name,
        baud,
        probed,
        &raw_frames,
        total_lines,
        verdict,
    );
    Ok(SniffResultDto {
        verdict,
        raw_frames,
        channels,
        diagnostic,
    })
}

/// The copy-pasteable device-report block (port, VID/PID, chip, baud, frames,
/// verdict) matching the GitHub `device-report.yml` issue template.
fn diagnostic_block(
    port_name: &str,
    baud: u32,
    probed: bool,
    raw_frames: &[String],
    total_lines: usize,
    verdict: SniffVerdict,
) -> String {
    let usb = serialport::available_ports().ok().and_then(|ports| {
        ports
            .into_iter()
            .find_map(|p| match (p.port_name == port_name, p.port_type) {
                (true, serialport::SerialPortType::UsbPort(usb)) => Some(usb),
                _ => None,
            })
    });
    let mut out = String::new();
    out.push_str("Droptime Logger — device scan\n");
    out.push_str(&format!("port: {port_name}\n"));
    match &usb {
        Some(u) => {
            let chip = match chip_for_vid(u.vid) {
                SerialChip::Ftdi => "ftdi",
                SerialChip::Cp210x => "cp210x",
                SerialChip::Ch340 => "ch340",
                SerialChip::Other => "other",
            };
            out.push_str(&format!(
                "vid:pid: {:04x}:{:04x} (chip: {chip})\n",
                u.vid, u.pid
            ));
            out.push_str(&format!(
                "manufacturer: {}\n",
                u.manufacturer.as_deref().unwrap_or("unknown")
            ));
            out.push_str(&format!(
                "product: {}\n",
                u.product.as_deref().unwrap_or("unknown")
            ));
        }
        None => out.push_str("vid:pid: unknown (not enumerated as USB)\n"),
    }
    out.push_str(&format!("baud: {baud}\n"));
    out.push_str(&format!(
        "probe: {}\n",
        if probed {
            "passive listen + one READ (read-only)"
        } else {
            "passive listen only"
        }
    ));
    out.push_str(&format!(
        "frames ({} shown, {} lines received):\n",
        raw_frames.len(),
        total_lines
    ));
    if raw_frames.is_empty() {
        out.push_str("  (none)\n");
    }
    for frame in raw_frames {
        out.push_str(&format!("  {frame}\n"));
    }
    let verdict_str = match verdict {
        SniffVerdict::Tc4 => "tc4",
        SniffVerdict::Unknown => "unknown",
        SniffVerdict::Silent => "silent",
    };
    out.push_str(&format!("verdict: {verdict_str}\n"));
    out
}

// ---------------------------------------------------------------------------
// Port preview (§7.2) — one at a time, on its own thread
// ---------------------------------------------------------------------------

struct PreviewWorker {
    stop_tx: mpsc::SyncSender<()>,
    handle: JoinHandle<()>,
}

/// The single preview slot (`preview_active` bookkeeping lives here; the
/// session-active refusal is enforced at the IPC layer which owns the Engine).
static PREVIEW: Mutex<Option<PreviewWorker>> = Mutex::new(None);

fn preview_slot() -> std::sync::MutexGuard<'static, Option<PreviewWorker>> {
    PREVIEW
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn start_preview(
    args: &StartPortPreviewArgs,
    on_event: PreviewEmitter,
) -> Result<(), LoggerError> {
    start_preview_with_timing(args, on_event, Tc4Timing::default())
}

pub(crate) fn start_preview_with_timing(
    args: &StartPortPreviewArgs,
    on_event: PreviewEmitter,
    timing: Tc4Timing,
) -> Result<(), LoggerError> {
    let mut slot = preview_slot();
    if let Some(worker) = slot.take() {
        if worker.handle.is_finished() {
            let _ = worker.handle.join(); // reap a preview that ended on its own
        } else {
            *slot = Some(worker);
            return Err(LoggerError::preview_active());
        }
    }

    let baud = args.baud.unwrap_or(DEFAULT_BAUD);
    let unit = args.unit.unwrap_or(TempUnitDto::F);
    let port = open_port(&args.port_name, baud, timing.chunk_timeout)?;
    let (stop_tx, stop_rx) = mpsc::sync_channel::<()>(1);
    let handle = std::thread::Builder::new()
        .name("tc4-preview".into())
        .spawn(move || run_preview(port, unit, timing, &stop_rx, &on_event))
        .map_err(|e| LoggerError::io(format!("failed to spawn preview thread: {e}")))?;
    *slot = Some(PreviewWorker { stop_tx, handle });
    Ok(())
}

pub fn stop_preview() -> Result<(), LoggerError> {
    stop_any_preview();
    Ok(())
}

/// Stop and join whatever preview is running (idempotent). Called by
/// `stop_port_preview` and by the engine before ANY `start_session` /
/// `resume_session` — a preview must never hold the port a session needs.
pub fn stop_any_preview() {
    let worker = preview_slot().take();
    if let Some(worker) = worker {
        let _ = worker.stop_tx.send(());
        let _ = worker.handle.join();
    }
}

fn run_preview(
    mut port: Box<dyn SerialPort>,
    unit: TempUnitDto,
    timing: Tc4Timing,
    stop_rx: &mpsc::Receiver<()>,
    on_event: &PreviewEmitter,
) {
    let mut reader = LineReader::new();
    match init_device(&mut *port, &mut reader, unit, &timing, stop_rx) {
        Some(true) => {}
        Some(false) | None => return,
    }
    let mut failures = 0u32;
    let mut next_poll = Instant::now();
    loop {
        let now = Instant::now();
        if now < next_poll {
            match stop_rx.recv_timeout(next_poll - now) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
        next_poll += timing.poll_interval;
        if next_poll < Instant::now() {
            next_poll = Instant::now();
        }
        match poll_once(&mut *port, &mut reader, &timing, stop_rx) {
            PollOutcome::Stopped => return,
            PollOutcome::Frame(frame) => {
                failures = 0;
                // Raw channels in 1-based TC4 order, unit-converted; a hard
                // 0.00 is how firmware reports a disabled/absent input.
                let channels: Vec<Option<f64>> = frame
                    .channels
                    .iter()
                    .map(|&v| (v != 0.0).then(|| to_fahrenheit(v, unit)))
                    .collect();
                on_event(PreviewEvent {
                    channels,
                    ambient_f: Some(to_fahrenheit(frame.ambient, unit)),
                    at_ms: now_ms(),
                });
            }
            PollOutcome::Failed => {
                failures += 1;
                if failures >= 3 {
                    return; // preview ends quietly; the wizard just re-scans
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

/// Serializes tests around the process-global preview slot: the pty preview
/// tests hold it while streaming, and every engine test that starts/resumes a
/// session holds it too, because `Engine::start_session`/`resume_session`
/// call [`stop_any_preview`] and would kill a concurrently-running preview
/// test's stream. Process-global state needs process-global test coordination.
#[cfg(test)]
pub(crate) fn preview_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_ports_enumerates_without_panicking() {
        // Hardware-independent smoke test: enumeration succeeds and every
        // USB row carries a chip classification consistent with `likely`.
        let ports = list_ports().expect("enumeration must not fail");
        for p in &ports {
            if let Some(chip) = p.chip {
                assert_eq!(p.likely, chip != SerialChip::Other);
                assert!(p.vid.is_some() && p.pid.is_some());
            } else {
                assert!(!p.likely, "non-USB ports are never 'likely'");
            }
        }
    }

    #[test]
    fn chip_classification_covers_the_known_bridges() {
        assert_eq!(chip_for_vid(0x0403), SerialChip::Ftdi);
        assert_eq!(chip_for_vid(0x10C4), SerialChip::Cp210x);
        assert_eq!(chip_for_vid(0x1A86), SerialChip::Ch340);
        assert_eq!(chip_for_vid(0x2341), SerialChip::Other); // bare Arduino VID
    }

    #[test]
    fn open_error_mapping_missing_vs_busy() {
        let missing = serialport::Error::new(serialport::ErrorKind::NoDevice, "gone");
        assert_eq!(
            map_open_error("/dev/x", &missing).code,
            crate::error::ErrorCode::PortError
        );

        let busy = serialport::Error::new(
            serialport::ErrorKind::Io(std::io::ErrorKind::PermissionDenied),
            "…",
        );
        let err = map_open_error("/dev/x", &busy);
        assert_eq!(err.code, crate::error::ErrorCode::PortBusy);
        assert!(
            err.message.contains("Artisan"),
            "busy hint names the usual culprit"
        );

        let textual_busy = serialport::Error::new(serialport::ErrorKind::Unknown, "Resource busy");
        assert_eq!(
            map_open_error("/dev/x", &textual_busy).code,
            crate::error::ErrorCode::PortBusy
        );
    }

    #[test]
    fn sniffing_a_missing_port_is_port_error_not_a_crash() {
        let err = sniff(&SniffPortArgs {
            port_name: "/dev/definitely-not-a-port".into(),
            baud: None,
            duration_ms: Some(50),
        })
        .unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::PortError);
    }

    #[cfg(unix)]
    mod pty {
        use super::super::super::tc4::emu::{EmuConfig, Emulator};
        use super::*;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        fn fast_timing() -> Tc4Timing {
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

        #[test]
        fn sniffer_classifies_a_polled_tc4_rig_via_the_read_probe() {
            // aArtisanQ only speaks when polled: the passive window is silent,
            // the single READ probe answers → verdict tc4.
            let emulator = Emulator::spawn(EmuConfig::default());
            let result = sniff(&SniffPortArgs {
                port_name: emulator.slave_path.clone(),
                baud: None,
                duration_ms: Some(200),
            })
            .unwrap();
            assert_eq!(result.verdict, SniffVerdict::Tc4);
            assert!(!result.raw_frames.is_empty());
            assert_eq!(result.channels, Some(vec![1, 2]), "bt+et on ch1/ch2");
            for needle in ["port:", "baud: 115200", "verdict: tc4", "READ"] {
                assert!(
                    result.diagnostic.contains(needle),
                    "diagnostic block missing {needle:?}:\n{}",
                    result.diagnostic
                );
            }
            // The probe is the ONLY thing the sniffer ever transmits.
            let sent = emulator.sent_commands();
            assert_eq!(sent, vec!["READ".to_string()]);
        }

        #[test]
        fn sniffer_reports_silent_when_nothing_answers() {
            let emulator = Emulator::spawn(EmuConfig::default());
            emulator.set_responsive(false);
            let result = sniff(&SniffPortArgs {
                port_name: emulator.slave_path.clone(),
                baud: None,
                duration_ms: Some(150),
            })
            .unwrap();
            assert_eq!(result.verdict, SniffVerdict::Silent);
            assert!(result.raw_frames.is_empty());
            assert!(result.diagnostic.contains("verdict: silent"));
        }

        #[test]
        fn preview_streams_frames_one_at_a_time_and_stops() {
            let _guard = preview_test_lock();
            // The emulator curve is °F; the preview inits the rig with
            // UNITS;C so the wire carries °C (413.6°F → 212.00°C) and the
            // preview must convert it back.
            let emulator = Emulator::spawn(EmuConfig {
                curve: vec![(0.0, 413.6)],
                bt_slot: 1,
                et_slot: 2,
                power_fields: false,
            });
            let frames: Arc<Mutex<Vec<PreviewEvent>>> = Arc::new(Mutex::new(Vec::new()));
            let count = Arc::new(AtomicUsize::new(0));
            let emitter: PreviewEmitter = {
                let frames = Arc::clone(&frames);
                let count = Arc::clone(&count);
                Box::new(move |ev| {
                    frames.lock().unwrap().push(ev);
                    count.fetch_add(1, Ordering::SeqCst);
                })
            };
            let args = StartPortPreviewArgs {
                port_name: emulator.slave_path.clone(),
                baud: None,
                unit: Some(TempUnitDto::C), // device reports °C; preview converts
            };
            start_preview_with_timing(&args, emitter, fast_timing()).unwrap();

            // ONE preview at a time.
            let second: PreviewEmitter = Box::new(|_| {});
            let err = start_preview_with_timing(&args, second, fast_timing()).unwrap_err();
            assert_eq!(err.code, crate::error::ErrorCode::PreviewActive);

            let deadline = Instant::now() + Duration::from_secs(10);
            while count.load(Ordering::SeqCst) < 3 {
                assert!(Instant::now() < deadline, "preview produced too few frames");
                std::thread::sleep(Duration::from_millis(5));
            }
            stop_preview().unwrap();

            let frames = frames.lock().unwrap();
            let first = &frames[0];
            // Wire 212.00°C → 413.6°F on channel 1; ET (= BT + 40°F) on
            // channel 2; ch3/4 report a hard 0.00 → None; ambient converted
            // °F→°C→°F losslessly.
            assert!((first.channels[0].unwrap() - 413.6).abs() < 0.2);
            assert!((first.channels[1].unwrap() - 453.6).abs() < 0.2);
            assert_eq!(first.channels[2], None);
            assert_eq!(first.channels[3], None);
            assert!((first.ambient_f.unwrap() - 72.0).abs() < 0.2);
            assert!(first.at_ms > 0);

            // Stopped previews release the slot: a new one may start.
            let third: PreviewEmitter = Box::new(|_| {});
            start_preview_with_timing(&args, third, fast_timing()).unwrap();
            stop_preview().unwrap();
        }
    }
}
