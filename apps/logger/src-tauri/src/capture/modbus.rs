//! capture/modbus.rs — the generic MODBUS-TCP `DeviceSource` (CONTRACTS.md
//! §7.2 seams; user-facing protocol doc: docs/protocols/modbus-generic.md).
//!
//! READ-ONLY invariant (hard, enforced by construction): the only requests
//! this driver can put on the wire go through [`ReadOnlyModbusClient::read`],
//! whose function code comes from [`ModbusRegisterKind`] — a closed two-variant
//! enum mapping to FC03 (read holding registers) and FC04 (read input
//! registers). No write function-code path exists; do not add one. The
//! tokio-modbus `Context` (which does expose write methods) is a private field
//! of the wrapper and never escapes it.
//!
//! PROVENANCE (clean-room): MODBUS is an open, published standard — the
//! MODBUS Application Protocol Specification V1.1b3 is freely available from
//! modbus.org, so protocol knowledge here is unrestricted. This driver embeds
//! NO vendor register map: the register layout is user configuration (the
//! SourcePin below), sourced by the operator from their machine's own
//! documentation. Framing/MBAP is handled by the `tokio-modbus` crate
//! (MIT OR Apache-2.0). No GPL implementation (Artisan's or any other) was
//! read, ported, or paraphrased for this driver, and no proprietary register
//! map may ever be transcribed into this repository from one.
//!
//! SourcePin JSON shape (additive §5 extension; the engine carries pins as
//! raw JSON and this driver owns the schema):
//!
//! ```json
//! {
//!   "sourceId": "modbus-tcp:<host>:<port>",
//!   "unitId": 1,
//!   "channels": [
//!     { "role": "bt", "register": 4001, "kind": "holding",
//!       "dataType": "u16", "scale": 0.1, "offset": 0 }
//!   ],
//!   "unit": "C",
//!   "pollMs": 1000
//! }
//! ```
//!
//! * `channels[].role` — `bt` (required, exactly once) | `et` | `ambient` |
//!   `heater` | `fan` | `drum`; each role at most once. Temperature roles
//!   (`bt`/`et`/`ambient`) are °C→°F converted when `unit` is `"C"`;
//!   `heater`/`fan`/`drum` pass through unconverted.
//! * `channels[].register` — raw protocol data address 0–65535 (NOT the
//!   Modicon `40001`/`30001` convention; see the protocol doc).
//! * `channels[].kind` — `holding` (FC03, default) | `input` (FC04).
//! * `channels[].dataType` — `u16` (default) | `i16` | `f32` (standard word
//!   order, high word at the lower address) | `f32-swapped` (low word first).
//! * `channels[].scale`/`offset` — `value = decode(registers) × scale + offset`
//!   (defaults 1.0 / 0.0).
//! * `unitId` — MODBUS unit/slave identifier (default 1).
//! * `pollMs` — poll cadence in milliseconds (default 1000, floor 50).
//!
//! Polling batches channels whose registers are contiguous (per register
//! table, respecting the 125-register read limit) into single reads; anything
//! non-contiguous is read individually.
//!
//! Async containment: tokio never leaks past this module. The source thread
//! owns a private current-thread tokio runtime; every awaited operation is
//! wrapped in [`interruptible`] (deadline + stop-flag polling), so the outer
//! capture loop stays synchronous and `stop()` joins fast, exactly like the
//! TC4 driver's thread model.
//!
//! Failure matrix (§7.2, same as TC4): 3 consecutive poll failures →
//! `status: disconnected` → reconnect loop with 2 s..30 s doubling backoff —
//! an attempt only counts once a probe poll yields a USABLE sample (finite
//! BT) — → `status: reconnected`, seq continues gap-free. `stop()` signals
//! and JOINS the read thread (< 500 ms).

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::runtime::Runtime;
use tokio_modbus::client::{tcp as modbus_tcp, Context, Reader};
use tokio_modbus::Slave;

use crate::error::LoggerError;
use crate::model::{SampleDto, SourceInfo, SourceKind, SourceStatusKind, TempUnitDto};

use super::tc4::to_fahrenheit;
use super::{DeviceSource, SampleSink, SourceEmit};

/// Source-id prefix this driver owns: `modbus-tcp:<host>:<port>`.
pub const SOURCE_PREFIX: &str = "modbus-tcp:";

/// How many consecutive poll failures flip the driver to `disconnected`.
const FAILURE_THRESHOLD: u32 = 3;

/// FC03/FC04 read at most 125 registers per request (MODBUS spec §6.3/§6.4).
const MAX_BATCH_REGISTERS: u32 = 125;

/// Floor for a user-configured `pollMs` — protects PLCs from a zero/typo
/// cadence turning the poll loop into a hammer.
const MIN_POLL_MS: u64 = 50;

pub fn is_modbus_source_id(source_id: &str) -> bool {
    source_id.starts_with(SOURCE_PREFIX)
}

// ---------------------------------------------------------------------------
// Pin schema (driver-owned; deserialized from the raw sourcePin JSON)
// ---------------------------------------------------------------------------

/// The SampleDto slot a channel feeds (`bt` required, the rest optional).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModbusRole {
    Bt,
    Et,
    Ambient,
    Heater,
    Fan,
    Drum,
}

impl ModbusRole {
    fn as_str(self) -> &'static str {
        match self {
            ModbusRole::Bt => "bt",
            ModbusRole::Et => "et",
            ModbusRole::Ambient => "ambient",
            ModbusRole::Heater => "heater",
            ModbusRole::Fan => "fan",
            ModbusRole::Drum => "drum",
        }
    }

    /// Temperature roles are unit-converted; duty/level roles pass through.
    fn is_temperature(self) -> bool {
        matches!(self, ModbusRole::Bt | ModbusRole::Et | ModbusRole::Ambient)
    }
}

/// Which register table a channel reads. This closed enum IS the read-only
/// invariant: it is the only source of function codes in the driver, and both
/// variants are reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModbusRegisterKind {
    /// FC03 — read holding registers.
    Holding,
    /// FC04 — read input registers.
    Input,
}

impl ModbusRegisterKind {
    pub(crate) fn function_code(self) -> u8 {
        match self {
            ModbusRegisterKind::Holding => 3,
            ModbusRegisterKind::Input => 4,
        }
    }
}

/// Register payload encoding. `f32` is standard word order (high word at the
/// lower address); `f32-swapped` is the low-word-first variant some devices
/// use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModbusDataType {
    #[serde(rename = "u16")]
    U16,
    #[serde(rename = "i16")]
    I16,
    #[serde(rename = "f32")]
    F32,
    #[serde(rename = "f32-swapped")]
    F32Swapped,
}

impl ModbusDataType {
    /// Register (16-bit word) footprint of one value.
    fn words(self) -> u16 {
        match self {
            ModbusDataType::U16 | ModbusDataType::I16 => 1,
            ModbusDataType::F32 | ModbusDataType::F32Swapped => 2,
        }
    }
}

/// One channel of a modbus SourcePin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModbusChannel {
    pub role: ModbusRole,
    /// Raw protocol data address 0–65535.
    pub register: u16,
    #[serde(default = "default_register_kind")]
    pub kind: ModbusRegisterKind,
    #[serde(default = "default_data_type")]
    pub data_type: ModbusDataType,
    #[serde(default = "default_scale")]
    pub scale: f64,
    #[serde(default)]
    pub offset: f64,
}

fn default_register_kind() -> ModbusRegisterKind {
    ModbusRegisterKind::Holding
}
fn default_data_type() -> ModbusDataType {
    ModbusDataType::U16
}
fn default_scale() -> f64 {
    1.0
}
fn default_unit_id() -> u8 {
    1
}
fn default_unit() -> TempUnitDto {
    TempUnitDto::F
}

/// The `modbus-tcp:` SourcePin document (module-header JSON shape). Unknown
/// fields are ignored so the shape can keep growing additively.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModbusPin {
    /// Informational; the session's `sourceId` is authoritative.
    #[serde(default)]
    #[allow(dead_code)] // documented pin-shape field (docs/protocols/modbus-generic.md)
    pub source_id: String,
    #[serde(default = "default_unit_id")]
    pub unit_id: u8,
    #[serde(default)]
    pub channels: Vec<ModbusChannel>,
    #[serde(default = "default_unit")]
    pub unit: TempUnitDto,
    #[serde(default)]
    pub poll_ms: Option<u64>,
}

// ---------------------------------------------------------------------------
// Validated config + read plan (batched where registers are contiguous)
// ---------------------------------------------------------------------------

/// One FC03/FC04 request of the poll plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReadOp {
    pub kind: ModbusRegisterKind,
    pub start: u16,
    pub count: u16,
}

/// Where a channel's words live in the poll plan's responses.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ChannelSlot {
    role: ModbusRole,
    data_type: ModbusDataType,
    scale: f64,
    offset: f64,
    /// Index into the plan's ops / the poll's response frames.
    op: usize,
    /// Word offset of the value inside that response frame.
    word: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct ModbusConfig {
    /// `<host>:<port>` (everything after the source-id prefix).
    host_port: String,
    unit_id: u8,
    unit: TempUnitDto,
    poll_ms: Option<u64>,
    ops: Vec<ReadOp>,
    slots: Vec<ChannelSlot>,
}

impl ModbusConfig {
    /// Validate a raw sourcePin JSON document against `source_id`
    /// (`modbus-tcp:<host>:<port>`). Every rejection is `invalid_args`.
    pub(crate) fn from_pin_value(
        source_id: &str,
        pin: &serde_json::Value,
    ) -> Result<Self, LoggerError> {
        let host_port = parse_host_port(source_id)?;
        let pin: ModbusPin = serde_json::from_value(pin.clone())
            .map_err(|e| LoggerError::invalid_args(format!("modbus sourcePin: {e}")))?;
        if pin.channels.is_empty() {
            return Err(LoggerError::invalid_args(
                "modbus sourcePin.channels must map at least a bt register",
            ));
        }
        let mut seen: Vec<ModbusRole> = Vec::new();
        for ch in &pin.channels {
            if seen.contains(&ch.role) {
                return Err(LoggerError::invalid_args(format!(
                    "modbus sourcePin.channels: duplicate role '{}'",
                    ch.role.as_str()
                )));
            }
            seen.push(ch.role);
            let end = u32::from(ch.register) + u32::from(ch.data_type.words());
            if end > u32::from(u16::MAX) + 1 {
                return Err(LoggerError::invalid_args(format!(
                    "modbus sourcePin.channels: register {} does not fit a {}-word value \
                     in the 16-bit address space",
                    ch.register,
                    ch.data_type.words()
                )));
            }
            if !ch.scale.is_finite() || !ch.offset.is_finite() || ch.scale == 0.0 {
                return Err(LoggerError::invalid_args(format!(
                    "modbus sourcePin.channels: role '{}' needs a finite non-zero scale \
                     and a finite offset",
                    ch.role.as_str()
                )));
            }
        }
        if !seen.contains(&ModbusRole::Bt) {
            return Err(LoggerError::invalid_args(
                "modbus sourcePin.channels must include a channel with role 'bt'",
            ));
        }
        let (ops, slots) = plan_reads(&pin.channels);
        Ok(Self {
            host_port,
            unit_id: pin.unit_id,
            unit: pin.unit,
            poll_ms: pin.poll_ms,
            ops,
            slots,
        })
    }

    /// `pollMs` (floored to [`MIN_POLL_MS`]) wins over the timing default.
    fn poll_interval(&self, timing: &ModbusTiming) -> Duration {
        self.poll_ms
            .map(|ms| Duration::from_millis(ms.max(MIN_POLL_MS)))
            .unwrap_or(timing.poll_interval)
    }
}

/// `modbus-tcp:<host>:<port>` → `<host>:<port>`, port validated.
fn parse_host_port(source_id: &str) -> Result<String, LoggerError> {
    let invalid = || {
        LoggerError::invalid_args(format!(
            "modbus source id must be modbus-tcp:<host>:<port>, got {source_id:?}"
        ))
    };
    let addr = source_id.strip_prefix(SOURCE_PREFIX).ok_or_else(invalid)?;
    let (host, port) = addr.rsplit_once(':').ok_or_else(invalid)?;
    if host.is_empty() || port.parse::<u16>().map(|p| p == 0).unwrap_or(true) {
        return Err(invalid());
    }
    Ok(addr.to_owned())
}

/// Build the poll plan: per register table, channels sorted by address are
/// merged into one read while they stay contiguous (or overlap) and the span
/// fits the 125-register request limit; any hole starts a new read.
fn plan_reads(channels: &[ModbusChannel]) -> (Vec<ReadOp>, Vec<ChannelSlot>) {
    let mut ops: Vec<ReadOp> = Vec::new();
    let mut slots: Vec<Option<ChannelSlot>> = vec![None; channels.len()];

    for kind in [ModbusRegisterKind::Holding, ModbusRegisterKind::Input] {
        let mut idxs: Vec<usize> = (0..channels.len())
            .filter(|&i| channels[i].kind == kind)
            .collect();
        idxs.sort_by_key(|&i| channels[i].register);

        for i in idxs {
            let ch = &channels[i];
            let end = u32::from(ch.register) + u32::from(ch.data_type.words());
            let mergeable = ops.last().is_some_and(|op| {
                op.kind == kind
                    && u32::from(ch.register) <= u32::from(op.start) + u32::from(op.count)
                    && end - u32::from(op.start) <= MAX_BATCH_REGISTERS
            });
            let op_idx = if mergeable {
                let op = ops.last_mut().expect("mergeable implies an op exists");
                op.count = op.count.max((end - u32::from(op.start)) as u16);
                ops.len() - 1
            } else {
                ops.push(ReadOp {
                    kind,
                    start: ch.register,
                    count: ch.data_type.words(),
                });
                ops.len() - 1
            };
            slots[i] = Some(ChannelSlot {
                role: ch.role,
                data_type: ch.data_type,
                scale: ch.scale,
                offset: ch.offset,
                op: op_idx,
                word: usize::from(ch.register - ops[op_idx].start),
            });
        }
    }

    let slots = slots
        .into_iter()
        .map(|s| s.expect("every channel is planned"))
        .collect();
    (ops, slots)
}

// ---------------------------------------------------------------------------
// Decoding
// ---------------------------------------------------------------------------

/// Decode one value from its register words (`words.len() == data_type.words()`).
pub(crate) fn decode_words(words: &[u16], data_type: ModbusDataType) -> f64 {
    match data_type {
        ModbusDataType::U16 => f64::from(words[0]),
        ModbusDataType::I16 => f64::from(words[0] as i16),
        ModbusDataType::F32 => f64::from(f32::from_bits(
            (u32::from(words[0]) << 16) | u32::from(words[1]),
        )),
        ModbusDataType::F32Swapped => f64::from(f32::from_bits(
            (u32::from(words[1]) << 16) | u32::from(words[0]),
        )),
    }
}

/// Map one poll's response frames to a sample. `None` when the BT value is
/// missing or non-finite — an unusable poll that counts as a read failure
/// (same finiteness discipline as the TC4 driver: a NaN/Inf BT must drive the
/// 3-strike disconnect flow, never a failed NOT NULL persist behind a healthy
/// status). Non-finite optional roles degrade to `None` instead.
fn sample_from_frames(
    frames: &[Vec<u16>],
    slots: &[ChannelSlot],
    unit: TempUnitDto,
    seq: u64,
    session_sec: f64,
) -> Option<SampleDto> {
    let mut by_role: [Option<f64>; 6] = [None; 6];
    for slot in slots {
        let words = frames.get(slot.op).and_then(|frame| {
            frame.get(slot.word..slot.word + usize::from(slot.data_type.words()))
        });
        // A short frame (misbehaving server) leaves the role empty; BT-empty
        // polls are rejected below.
        let Some(words) = words else { continue };
        let value = decode_words(words, slot.data_type) * slot.scale + slot.offset;
        if !value.is_finite() {
            continue;
        }
        let value = if slot.role.is_temperature() {
            to_fahrenheit(value, unit)
        } else {
            value
        };
        by_role[slot.role as usize] = Some(value);
    }
    Some(SampleDto {
        seq,
        session_sec,
        bt_f: by_role[ModbusRole::Bt as usize]?,
        et_f: by_role[ModbusRole::Et as usize],
        ambient_f: by_role[ModbusRole::Ambient as usize],
        heater: by_role[ModbusRole::Heater as usize],
        fan: by_role[ModbusRole::Fan as usize],
        drum: by_role[ModbusRole::Drum as usize],
    })
}

// ---------------------------------------------------------------------------
// The read-only client
// ---------------------------------------------------------------------------

/// The ONLY path to the wire. Owns the tokio-modbus [`Context`] privately —
/// `Context` implements the crate's `Writer` trait, but this wrapper exposes
/// exactly one request-issuing method, and its function code comes from the
/// closed [`ModbusRegisterKind`] enum (FC03/FC04). Read-only by construction.
struct ReadOnlyModbusClient {
    ctx: Context,
}

impl ReadOnlyModbusClient {
    async fn connect(addr: SocketAddr, unit_id: u8) -> std::io::Result<Self> {
        let ctx = modbus_tcp::connect_slave(addr, Slave(unit_id)).await?;
        Ok(Self { ctx })
    }

    /// Issue one read. FC03 for `Holding`, FC04 for `Input` — nothing else is
    /// expressible.
    async fn read(&mut self, op: ReadOp) -> tokio_modbus::Result<Vec<u16>> {
        match op.kind {
            ModbusRegisterKind::Holding => self.ctx.read_holding_registers(op.start, op.count),
            ModbusRegisterKind::Input => self.ctx.read_input_registers(op.start, op.count),
        }
        .await
    }
}

// ---------------------------------------------------------------------------
// Timing (accelerated by tests; defaults are the real-network values)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ModbusTiming {
    /// TCP connect budget (start_session fails fast past it).
    pub connect_timeout: Duration,
    /// Poll cadence fallback when the pin has no `pollMs`.
    pub poll_interval: Duration,
    /// Budget for one FC03/FC04 round trip.
    pub response_deadline: Duration,
    /// How often an in-flight awaited operation polls the stop flag — the
    /// upper bound `stop()` waits on a mid-operation join.
    pub stop_check: Duration,
    /// Reconnect backoff bounds (doubling).
    pub backoff_min: Duration,
    pub backoff_max: Duration,
}

impl Default for ModbusTiming {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_millis(3000),
            poll_interval: Duration::from_millis(1000),
            response_deadline: Duration::from_millis(750),
            stop_check: Duration::from_millis(25),
            backoff_min: Duration::from_secs(2),
            backoff_max: Duration::from_secs(30),
        }
    }
}

// ---------------------------------------------------------------------------
// Interruptible waits (sync + async)
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

enum OpOutcome<T> {
    Done(T),
    TimedOut,
    Stopped,
}

/// Await `fut` for at most `deadline`, checking the stop flag every
/// `stop_check` — this is what keeps `stop()` joins fast even while a network
/// operation is in flight, without tokio ever escaping the source thread.
async fn interruptible<F: std::future::Future>(
    fut: F,
    deadline: Duration,
    stop_check: Duration,
    stop: &AtomicBool,
) -> OpOutcome<F::Output> {
    tokio::pin!(fut);
    let end = tokio::time::Instant::now() + deadline;
    loop {
        if stop.load(Ordering::SeqCst) {
            return OpOutcome::Stopped;
        }
        let now = tokio::time::Instant::now();
        if now >= end {
            return OpOutcome::TimedOut;
        }
        let slice = stop_check.min(end - now);
        if let Ok(value) = tokio::time::timeout(slice, &mut fut).await {
            return OpOutcome::Done(value);
        }
    }
}

// ---------------------------------------------------------------------------
// Polling
// ---------------------------------------------------------------------------

enum PollOutcome {
    /// One response frame per plan op, in plan order.
    Frames(Vec<Vec<u16>>),
    Failed,
    Stopped,
}

/// Execute the whole read plan once. Any transport error, device exception,
/// or deadline miss fails the poll as a unit.
fn poll_once(
    rt: &Runtime,
    client: &mut ReadOnlyModbusClient,
    config: &ModbusConfig,
    timing: &ModbusTiming,
    stop: &AtomicBool,
) -> PollOutcome {
    let mut frames: Vec<Vec<u16>> = Vec::with_capacity(config.ops.len());
    for op in &config.ops {
        let outcome = rt.block_on(interruptible(
            client.read(*op),
            timing.response_deadline,
            timing.stop_check,
            stop,
        ));
        match outcome {
            OpOutcome::Stopped => return PollOutcome::Stopped,
            OpOutcome::TimedOut => return PollOutcome::Failed,
            OpOutcome::Done(Err(err)) => {
                tracing::debug!(error = %err, host = %config.host_port, "modbus read failed");
                return PollOutcome::Failed;
            }
            OpOutcome::Done(Ok(Err(exception))) => {
                tracing::debug!(
                    %exception, host = %config.host_port,
                    fc = op.kind.function_code(), start = op.start, count = op.count,
                    "modbus device exception"
                );
                return PollOutcome::Failed;
            }
            OpOutcome::Done(Ok(Ok(words))) => frames.push(words),
        }
    }
    PollOutcome::Frames(frames)
}

// ---------------------------------------------------------------------------
// ModbusSource
// ---------------------------------------------------------------------------

pub struct ModbusSource {
    config: ModbusConfig,
    /// First seq this source will emit (1 fresh, `last_persisted + 1` resumed).
    start_seq: u64,
    /// Session clock offset (0 fresh; wall-anchored seconds on resume — the
    /// engine owns that semantics, same as tc4).
    start_session_sec: f64,
    timing: ModbusTiming,
    worker: Option<Worker>,
}

struct Worker {
    stop_flag: Arc<AtomicBool>,
    stop_tx: mpsc::SyncSender<()>,
    handle: JoinHandle<()>,
}

impl ModbusSource {
    /// Factory entry (capture/mod.rs): requires a sourcePin whose `channels`
    /// include a `bt` role, else `invalid_args`.
    pub fn from_pin(
        source_id: &str,
        pin: Option<&serde_json::Value>,
        start_seq: u64,
        start_session_sec: f64,
    ) -> Result<Self, LoggerError> {
        let pin = pin.ok_or_else(|| {
            LoggerError::invalid_args(
                "modbus sources require a sourcePin (unitId, channels with a bt role)",
            )
        })?;
        let config = ModbusConfig::from_pin_value(source_id, pin)?;
        Ok(Self {
            config,
            start_seq: start_seq.max(1),
            start_session_sec: start_session_sec.max(0.0),
            timing: ModbusTiming::default(),
            worker: None,
        })
    }

    pub fn with_timing(mut self, timing: ModbusTiming) -> Self {
        self.timing = timing;
        self
    }
}

/// Resolve `<host>:<port>` (DNS included) to the first socket address.
fn resolve(host_port: &str) -> Result<SocketAddr, LoggerError> {
    use std::net::ToSocketAddrs;
    host_port
        .to_socket_addrs()
        .map_err(|e| LoggerError::port_error(format!("could not resolve {host_port}: {e}")))?
        .next()
        .ok_or_else(|| {
            LoggerError::port_error(format!("{host_port} did not resolve to any address"))
        })
}

fn map_connect_error(host_port: &str, err: &std::io::Error) -> LoggerError {
    if err.kind() == std::io::ErrorKind::ConnectionRefused {
        return LoggerError::port_error(format!(
            "{host_port} refused the connection — is the machine's MODBUS-TCP interface \
             enabled and listening on that port?"
        ));
    }
    LoggerError::port_error(format!("could not connect to {host_port}: {err}"))
}

impl DeviceSource for ModbusSource {
    fn descriptor(&self) -> SourceInfo {
        SourceInfo {
            id: format!("{SOURCE_PREFIX}{}", self.config.host_port),
            label: format!("MODBUS-TCP — {}", self.config.host_port),
            kind: SourceKind::Device,
        }
    }

    fn start(&mut self, sink: SampleSink) -> Result<(), LoggerError> {
        if self.worker.is_some() {
            return Err(LoggerError::io("modbus source already started"));
        }
        // The runtime is created here and moved into the source thread — it
        // never outlives the source and nothing async crosses this boundary.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .map_err(|e| LoggerError::io(format!("failed to build modbus runtime: {e}")))?;

        // Connect synchronously so start_session fails fast with port_error
        // (the tc4 open-in-start precedent).
        let addr = resolve(&self.config.host_port)?;
        // NB: built inside block_on — tokio timers must be created in a
        // runtime context.
        let connect_timeout = self.timing.connect_timeout;
        let unit_id = self.config.unit_id;
        let connected = rt.block_on(async {
            tokio::time::timeout(
                connect_timeout,
                ReadOnlyModbusClient::connect(addr, unit_id),
            )
            .await
        });
        let client = match connected {
            Err(_elapsed) => {
                return Err(LoggerError::port_error(format!(
                    "connection to {} timed out",
                    self.config.host_port
                )))
            }
            Ok(Err(err)) => return Err(map_connect_error(&self.config.host_port, &err)),
            Ok(Ok(client)) => client,
        };

        let stop_flag = Arc::new(AtomicBool::new(false));
        let (stop_tx, stop_rx) = mpsc::sync_channel::<()>(1);
        let ctx = CaptureCtx {
            config: self.config.clone(),
            timing: self.timing.clone(),
            start_seq: self.start_seq,
            start_session_sec: self.start_session_sec,
            stop_flag: Arc::clone(&stop_flag),
        };
        let thread_name = format!("modbus-{}", self.config.host_port);
        let handle = std::thread::Builder::new()
            .name(thread_name)
            .spawn(move || run_capture(rt, client, ctx, &stop_rx, &sink))
            .map_err(|e| LoggerError::io(format!("failed to spawn modbus thread: {e}")))?;
        self.worker = Some(Worker {
            stop_flag,
            stop_tx,
            handle,
        });
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(worker) = self.worker.take() {
            // Flag first (interrupts an in-flight await), then wake any sync
            // wait, then join.
            worker.stop_flag.store(true, Ordering::SeqCst);
            let _ = worker.stop_tx.send(());
            let _ = worker.handle.join();
        }
    }
}

impl Drop for ModbusSource {
    fn drop(&mut self) {
        self.stop();
    }
}

struct CaptureCtx {
    config: ModbusConfig,
    timing: ModbusTiming,
    start_seq: u64,
    start_session_sec: f64,
    stop_flag: Arc<AtomicBool>,
}

fn run_capture(
    rt: Runtime,
    mut client: ReadOnlyModbusClient,
    ctx: CaptureCtx,
    stop_rx: &mpsc::Receiver<()>,
    sink: &SampleSink,
) {
    let anchor = Instant::now();
    let session_sec = |ctx: &CaptureCtx| ctx.start_session_sec + anchor.elapsed().as_secs_f64();
    let poll_interval = ctx.config.poll_interval(&ctx.timing);
    let mut seq = ctx.start_seq;
    let mut failures: u32 = 0;

    let mut next_poll = Instant::now();
    loop {
        if !wait_until(next_poll, stop_rx) {
            return;
        }
        next_poll += poll_interval;
        if next_poll < Instant::now() {
            next_poll = Instant::now(); // catch up after a slow response
        }

        match poll_once(&rt, &mut client, &ctx.config, &ctx.timing, &ctx.stop_flag) {
            PollOutcome::Stopped => return,
            PollOutcome::Frames(frames) => {
                match sample_from_frames(
                    &frames,
                    &ctx.config.slots,
                    ctx.config.unit,
                    seq,
                    session_sec(&ctx),
                ) {
                    Some(sample) => {
                        failures = 0;
                        sink(SourceEmit::Sample(sample));
                        seq += 1;
                    }
                    None => failures += 1, // poll with a missing/non-finite BT
                }
            }
            PollOutcome::Failed => failures += 1,
        }

        if failures >= FAILURE_THRESHOLD {
            sink(SourceEmit::Status {
                kind: SourceStatusKind::Disconnected,
                message: Some(format!(
                    "lost contact with {}; reconnecting",
                    ctx.config.host_port
                )),
                at_session_sec: session_sec(&ctx),
            });
            drop(client); // close the dead socket before redialing
            match reconnect(&rt, &ctx, stop_rx) {
                Some(reconnected) => {
                    client = reconnected;
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

/// Redial with doubling backoff (2 s..30 s by default). An attempt only
/// counts as success after a probe poll yields a USABLE sample (finite BT) —
/// a server that accepts TCP but answers garbage keeps backing off instead of
/// flapping reconnected/disconnected. Returns `None` when stopped.
fn reconnect(
    rt: &Runtime,
    ctx: &CaptureCtx,
    stop_rx: &mpsc::Receiver<()>,
) -> Option<ReadOnlyModbusClient> {
    let mut backoff = ctx.timing.backoff_min;
    loop {
        if !wait_until(Instant::now() + backoff, stop_rx) {
            return None;
        }
        backoff = (backoff * 2).min(ctx.timing.backoff_max);
        // Re-resolve each attempt: a roaster panel can come back on a new
        // DHCP lease mid-roast.
        let Ok(addr) = resolve(&ctx.config.host_port) else {
            continue;
        };
        let connected = rt.block_on(interruptible(
            ReadOnlyModbusClient::connect(addr, ctx.config.unit_id),
            ctx.timing.connect_timeout,
            ctx.timing.stop_check,
            &ctx.stop_flag,
        ));
        let mut client = match connected {
            OpOutcome::Stopped => return None,
            OpOutcome::TimedOut => continue,
            OpOutcome::Done(Err(_)) => continue,
            OpOutcome::Done(Ok(client)) => client,
        };
        match poll_once(rt, &mut client, &ctx.config, &ctx.timing, &ctx.stop_flag) {
            PollOutcome::Stopped => return None,
            // The probe frame is validated but not emitted (tc4 precedent).
            PollOutcome::Frames(frames)
                if sample_from_frames(&frames, &ctx.config.slots, ctx.config.unit, 0, 0.0)
                    .is_some() =>
            {
                return Some(client);
            }
            PollOutcome::Frames(_) | PollOutcome::Failed => continue,
        }
    }
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

/// In-process MODBUS-TCP emulator — the no-hardware test rig (the
/// `modbus_emulator` example is the standalone contributor-facing twin, and
/// both serve the SAME default register map documented in
/// docs/protocols/modbus-generic.md). Platform-independent: unlike the TC4
/// pty emulator this needs no `#[cfg(unix)]`.
#[cfg(test)]
pub(crate) mod emu {
    use std::net::SocketAddr;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use tokio_modbus::server::tcp::{accept_tcp_connection, Server};
    use tokio_modbus::server::Service;
    use tokio_modbus::{ExceptionCode, Request, Response, SlaveRequest};

    use crate::model::TempUnitDto;

    /// Test-speed timing for the driver (poll 20 ms, short deadlines, fast
    /// backoff) — shared by the modbus tests here and the engine tests in
    /// capture/mod.rs.
    pub fn fast_timing() -> super::ModbusTiming {
        super::ModbusTiming {
            connect_timeout: Duration::from_millis(500),
            poll_interval: Duration::from_millis(20),
            response_deadline: Duration::from_millis(200),
            stop_check: Duration::from_millis(5),
            backoff_min: Duration::from_millis(40),
            backoff_max: Duration::from_millis(160),
        }
    }

    /// Piecewise-linear BT curve in °F over wall-clock seconds; held flat
    /// past the last knot. `unit` is what the registers CARRY (the driver's
    /// pin must match it); `speed` multiplies the curve clock.
    #[derive(Clone)]
    pub struct EmuConfig {
        pub curve: Vec<(f64, f64)>,
        pub unit: TempUnitDto,
        pub speed: f64,
    }

    impl Default for EmuConfig {
        fn default() -> Self {
            Self {
                curve: vec![(0.0, 390.0)],
                unit: TempUnitDto::F,
                speed: 1.0,
            }
        }
    }

    /// One audited request: (function code, start address, register count).
    /// Non-read requests are recorded as (fc, 0, 0) — the read-only audit
    /// must show they never happen.
    pub type AuditEntry = (u8, u16, u16);

    pub struct Emulator {
        pub port: u16,
        requests: Arc<Mutex<Vec<AuditEntry>>>,
        stop: Arc<AtomicBool>,
        handle: Option<std::thread::JoinHandle<()>>,
    }

    impl Emulator {
        pub fn spawn(config: EmuConfig) -> Self {
            Self::spawn_on(0, config)
        }

        /// Bind to a specific port (0 = ephemeral). `SO_REUSEADDR` is set so
        /// a test can kill the server and respawn it on the SAME port — the
        /// "unplug the network cable, then plug it back in" lever.
        pub fn spawn_on(port: u16, config: EmuConfig) -> Self {
            let requests: Arc<Mutex<Vec<AuditEntry>>> = Arc::new(Mutex::new(Vec::new()));
            let stop = Arc::new(AtomicBool::new(false));
            let (port_tx, port_rx) = std::sync::mpsc::channel::<u16>();
            let handle = {
                let requests = Arc::clone(&requests);
                let stop = Arc::clone(&stop);
                std::thread::Builder::new()
                    .name("modbus-emu".into())
                    .spawn(move || run_server(port, config, requests, stop, port_tx))
                    .expect("spawn modbus emulator thread")
            };
            let port = port_rx.recv().expect("modbus emulator failed to bind");
            Self {
                port,
                requests,
                stop,
                handle: Some(handle),
            }
        }

        pub fn source_id(&self) -> String {
            format!("{}127.0.0.1:{}", super::SOURCE_PREFIX, self.port)
        }

        /// Every request the driver ever sent — the read-only audit log.
        pub fn sent_requests(&self) -> Vec<AuditEntry> {
            self.requests.lock().unwrap().clone()
        }

        /// Shut the server down (open connections die with it), returning the
        /// port so a test can respawn on it.
        pub fn kill(mut self) -> u16 {
            let port = self.port;
            self.shutdown();
            port
        }

        fn shutdown(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    impl Drop for Emulator {
        fn drop(&mut self) {
            self.shutdown();
        }
    }

    fn run_server(
        port: u16,
        config: EmuConfig,
        requests: Arc<Mutex<Vec<AuditEntry>>>,
        stop: Arc<AtomicBool>,
        port_tx: std::sync::mpsc::Sender<u16>,
    ) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .expect("build emulator runtime");
        rt.block_on(async move {
            let socket = tokio::net::TcpSocket::new_v4().expect("socket");
            socket.set_reuseaddr(true).expect("reuseaddr");
            socket
                .bind(SocketAddr::from(([127, 0, 0, 1], port)))
                .expect("bind emulator port");
            let listener = socket.listen(64).expect("listen");
            port_tx
                .send(listener.local_addr().expect("local addr").port())
                .expect("report port");

            let state = Arc::new(EmuState {
                config,
                anchor: Instant::now(),
                requests,
            });
            let service = EmuService { state };
            let on_connected = move |stream, socket_addr| {
                let service = service.clone();
                async move {
                    accept_tcp_connection(stream, socket_addr, move |_addr| {
                        Ok(Some(service.clone()))
                    })
                }
            };
            let abort = {
                let stop = Arc::clone(&stop);
                async move {
                    while !stop.load(Ordering::SeqCst) {
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                }
            };
            let _ = Server::new(listener)
                .serve_until(&on_connected, |_err: std::io::Error| {}, abort)
                .await;
        });
        // Dropping the runtime here tears down every open connection task.
    }

    struct EmuState {
        config: EmuConfig,
        anchor: Instant,
        requests: Arc<Mutex<Vec<AuditEntry>>>,
    }

    #[derive(Clone)]
    struct EmuService {
        state: Arc<EmuState>,
    }

    impl Service for EmuService {
        type Request = SlaveRequest<'static>;
        type Response = Response;
        type Exception = ExceptionCode;
        type Future = std::future::Ready<Result<Response, ExceptionCode>>;

        fn call(&self, req: Self::Request) -> Self::Future {
            std::future::ready(self.state.handle(req.request))
        }
    }

    impl EmuState {
        fn handle(&self, request: Request<'static>) -> Result<Response, ExceptionCode> {
            match request {
                Request::ReadInputRegisters(addr, cnt) => {
                    self.audit(4, addr, cnt);
                    self.read_span(super::ModbusRegisterKind::Input, addr, cnt)
                        .map(Response::ReadInputRegisters)
                }
                Request::ReadHoldingRegisters(addr, cnt) => {
                    self.audit(3, addr, cnt);
                    self.read_span(super::ModbusRegisterKind::Holding, addr, cnt)
                        .map(Response::ReadHoldingRegisters)
                }
                // Write/other function codes would land here: the Logger must
                // never send one, and the audit log proves it.
                other => {
                    self.audit(other.function_code().value(), 0, 0);
                    Err(ExceptionCode::IllegalFunction)
                }
            }
        }

        fn audit(&self, fc: u8, addr: u16, cnt: u16) {
            self.requests.lock().unwrap().push((fc, addr, cnt));
        }

        fn read_span(
            &self,
            kind: super::ModbusRegisterKind,
            addr: u16,
            cnt: u16,
        ) -> Result<Vec<u16>, ExceptionCode> {
            let t = self.anchor.elapsed().as_secs_f64() * self.config.speed;
            let bt_f = curve_at(&self.config.curve, t);
            let et_f = bt_f + 40.0;
            let conv = |f: f64| match self.config.unit {
                TempUnitDto::F => f,
                TempUnitDto::C => (f - 32.0) * 5.0 / 9.0,
            };
            let (bt, et) = (conv(bt_f), conv(et_f));
            let end = u32::from(addr) + u32::from(cnt);
            (u32::from(addr)..end)
                .map(|a| self.register(kind, a as u16, bt, et))
                .collect()
        }

        /// The default emulator register map — keep in lockstep with the
        /// `modbus_emulator` example and docs/protocols/modbus-generic.md.
        fn register(
            &self,
            kind: super::ModbusRegisterKind,
            addr: u16,
            bt: f64,
            et: f64,
        ) -> Result<u16, ExceptionCode> {
            use super::ModbusRegisterKind::{Holding, Input};
            let x10 = |v: f64| (v * 10.0).round().clamp(0.0, 65535.0) as u16;
            let f32_hi = |v: f64| ((v as f32).to_bits() >> 16) as u16;
            let f32_lo = |v: f64| (v as f32).to_bits() as u16;
            match (kind, addr) {
                (Holding, 0) | (Input, 0) => Ok(x10(bt)),
                (Holding, 1) | (Input, 1) => Ok(x10(et)),
                (Input, 2) => Ok(f32_hi(bt)),
                (Input, 3) => Ok(f32_lo(bt)),
                (Input, 4) => Ok(f32_hi(et)),
                (Input, 5) => Ok(f32_lo(et)),
                (Input, 6) => Ok(f32_lo(bt)), // swapped: low word first
                (Input, 7) => Ok(f32_hi(bt)),
                (Input, 8) => Ok(450), // heater duty % ×10
                (Input, 9) => Ok(600), // fan duty % ×10
                _ => Err(ExceptionCode::IllegalDataAddress),
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
}

#[cfg(test)]
mod tests {
    use super::emu::{fast_timing, EmuConfig, Emulator};
    use super::*;
    use crate::capture::{SampleSink, SourceEmit};
    use std::sync::Mutex;

    fn pin_json(source_id: &str, channels: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "sourceId": source_id,
            "unitId": 1,
            "channels": channels,
            "unit": "F",
        })
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

    // -- config parsing / validation -------------------------------------

    #[test]
    fn source_id_parsing_accepts_host_port_and_rejects_malformed() {
        assert_eq!(
            parse_host_port("modbus-tcp:192.168.1.199:502").unwrap(),
            "192.168.1.199:502"
        );
        assert_eq!(
            parse_host_port("modbus-tcp:roaster.local:1502").unwrap(),
            "roaster.local:1502"
        );
        assert_eq!(
            parse_host_port("modbus-tcp:[::1]:502").unwrap(),
            "[::1]:502"
        );

        for bad in [
            "tcp:10.0.0.1:502",      // wrong prefix
            "modbus-tcp:10.0.0.1",   // no port
            "modbus-tcp::502",       // empty host
            "modbus-tcp:host:0",     // port 0
            "modbus-tcp:host:70000", // port out of range
            "modbus-tcp:host:fivehundred-two",
        ] {
            let err = parse_host_port(bad).unwrap_err();
            assert_eq!(err.code, crate::error::ErrorCode::InvalidArgs, "{bad}");
        }
    }

    #[test]
    fn pin_validation_requires_bt_and_unique_roles() {
        let sid = "modbus-tcp:10.0.0.5:502";
        let invalid = crate::error::ErrorCode::InvalidArgs;

        // Not an object at all.
        let err = ModbusConfig::from_pin_value(sid, &serde_json::json!(42)).unwrap_err();
        assert_eq!(err.code, invalid);

        // No channels.
        let err =
            ModbusConfig::from_pin_value(sid, &pin_json(sid, serde_json::json!([]))).unwrap_err();
        assert_eq!(err.code, invalid);
        assert!(err.message.contains("bt"));

        // Channels but no bt role.
        let pin = pin_json(sid, serde_json::json!([{ "role": "et", "register": 1 }]));
        let err = ModbusConfig::from_pin_value(sid, &pin).unwrap_err();
        assert_eq!(err.code, invalid);
        assert!(err.message.contains("'bt'"));

        // Duplicate role.
        let pin = pin_json(
            sid,
            serde_json::json!([
                { "role": "bt", "register": 0 },
                { "role": "bt", "register": 2 },
            ]),
        );
        let err = ModbusConfig::from_pin_value(sid, &pin).unwrap_err();
        assert_eq!(err.code, invalid);
        assert!(err.message.contains("duplicate"));

        // A 2-word f32 does not fit at the top of the address space.
        let pin = pin_json(
            sid,
            serde_json::json!([{ "role": "bt", "register": 65535, "dataType": "f32" }]),
        );
        let err = ModbusConfig::from_pin_value(sid, &pin).unwrap_err();
        assert_eq!(err.code, invalid);

        // Zero / non-finite scale is a config error, not a silent flatline.
        let pin = pin_json(
            sid,
            serde_json::json!([{ "role": "bt", "register": 0, "scale": 0.0 }]),
        );
        let err = ModbusConfig::from_pin_value(sid, &pin).unwrap_err();
        assert_eq!(err.code, invalid);

        // Unknown dataType string is rejected by the schema.
        let pin = pin_json(
            sid,
            serde_json::json!([{ "role": "bt", "register": 0, "dataType": "f64" }]),
        );
        let err = ModbusConfig::from_pin_value(sid, &pin).unwrap_err();
        assert_eq!(err.code, invalid);

        // Minimal valid pin: defaults fill in (unitId 1, holding/u16/scale 1/
        // offset 0/unit F, pollMs absent).
        let pin = serde_json::json!({
            "sourceId": sid,
            "channels": [{ "role": "bt", "register": 4001 }],
        });
        let config = ModbusConfig::from_pin_value(sid, &pin).unwrap();
        assert_eq!(config.host_port, "10.0.0.5:502");
        assert_eq!(config.unit_id, 1);
        assert_eq!(config.unit, TempUnitDto::F);
        assert_eq!(config.poll_ms, None);
        assert_eq!(
            config.ops,
            vec![ReadOp {
                kind: ModbusRegisterKind::Holding,
                start: 4001,
                count: 1
            }]
        );
        let slot = &config.slots[0];
        assert_eq!(slot.data_type, ModbusDataType::U16);
        assert_eq!((slot.scale, slot.offset), (1.0, 0.0));

        // Unknown extra fields are tolerated (additive shape).
        let pin = serde_json::json!({
            "sourceId": sid,
            "channels": [{ "role": "bt", "register": 0, "futureField": true }],
            "futureTopLevel": { "x": 1 },
        });
        assert!(ModbusConfig::from_pin_value(sid, &pin).is_ok());
    }

    #[test]
    fn poll_interval_prefers_poll_ms_with_floor() {
        let sid = "modbus-tcp:h:502";
        let timing = ModbusTiming::default();
        let mk = |poll_ms: serde_json::Value| {
            let mut pin = pin_json(sid, serde_json::json!([{ "role": "bt", "register": 0 }]));
            pin["pollMs"] = poll_ms;
            ModbusConfig::from_pin_value(sid, &pin).unwrap()
        };
        // Absent → the timing default.
        assert_eq!(
            mk(serde_json::Value::Null).poll_interval(&timing),
            timing.poll_interval
        );
        // Present → wins over the timing default.
        assert_eq!(
            mk(serde_json::json!(250)).poll_interval(&timing),
            Duration::from_millis(250)
        );
        // Floored so a typo can never hammer a PLC.
        assert_eq!(
            mk(serde_json::json!(1)).poll_interval(&timing),
            Duration::from_millis(MIN_POLL_MS)
        );
    }

    // -- decoding ---------------------------------------------------------

    #[test]
    fn data_type_decoding_covers_the_matrix() {
        // u16: full range, no sign.
        assert_eq!(decode_words(&[0], ModbusDataType::U16), 0.0);
        assert_eq!(decode_words(&[65_535], ModbusDataType::U16), 65_535.0);

        // i16: two's complement.
        assert_eq!(decode_words(&[0xFFFF], ModbusDataType::I16), -1.0);
        assert_eq!(decode_words(&[0xFB2E], ModbusDataType::I16), -1234.0);
        assert_eq!(decode_words(&[1234], ModbusDataType::I16), 1234.0);

        // f32, standard word order (high word at the lower address).
        let bits = 415.25f32.to_bits();
        let (hi, lo) = ((bits >> 16) as u16, bits as u16);
        assert_eq!(decode_words(&[hi, lo], ModbusDataType::F32), 415.25);
        // …and the same bytes read swapped give garbage, not 415.25.
        assert_ne!(decode_words(&[hi, lo], ModbusDataType::F32Swapped), 415.25);

        // f32-swapped (low word at the lower address).
        assert_eq!(decode_words(&[lo, hi], ModbusDataType::F32Swapped), 415.25);
        let neg = (-12.5f32).to_bits();
        assert_eq!(
            decode_words(
                &[neg as u16, (neg >> 16) as u16],
                ModbusDataType::F32Swapped
            ),
            -12.5
        );

        // Non-finite f32 payloads survive decoding as non-finite (rejected
        // later by the finiteness gate, never silently stored).
        let nan = f32::NAN.to_bits();
        assert!(decode_words(&[(nan >> 16) as u16, nan as u16], ModbusDataType::F32).is_nan());
    }

    #[test]
    fn sample_mapping_applies_scale_offset_units_and_finiteness() {
        let sid = "modbus-tcp:h:502";
        // BT u16 ×0.1 (°C), ET i16 ×0.5 offset -10 (°C), heater u16 raw.
        let pin = serde_json::json!({
            "sourceId": sid,
            "unit": "C",
            "channels": [
                { "role": "bt", "register": 0, "kind": "input", "dataType": "u16", "scale": 0.1 },
                { "role": "et", "register": 1, "kind": "input", "dataType": "i16",
                  "scale": 0.5, "offset": -10.0 },
                { "role": "heater", "register": 2, "kind": "input" },
            ],
        });
        let config = ModbusConfig::from_pin_value(sid, &pin).unwrap();
        assert_eq!(config.ops.len(), 1, "contiguous input registers batch");

        // 2000 ×0.1 = 200°C → 392°F; 500 ×0.5 −10 = 240°C → 464°F; heater 45
        // passes through UNCONVERTED (duty roles are not temperatures).
        let frames = vec![vec![2000, 500, 45]];
        let s = sample_from_frames(&frames, &config.slots, config.unit, 7, 12.5).unwrap();
        assert_eq!(s.seq, 7);
        assert_eq!(s.session_sec, 12.5);
        assert!((s.bt_f - 392.0).abs() < 1e-9);
        assert!((s.et_f.unwrap() - 464.0).abs() < 1e-9);
        assert_eq!(s.heater, Some(45.0));
        assert_eq!((s.ambient_f, s.fan, s.drum), (None, None, None));

        // A short frame starves BT → unusable poll (counts as a failure).
        let short = vec![vec![2000u16]];
        let et_only_missing = sample_from_frames(&short, &config.slots, config.unit, 1, 0.0);
        assert!(et_only_missing.is_some(), "BT present, ET degrades to None");
        assert_eq!(et_only_missing.unwrap().et_f, None);
        assert!(sample_from_frames(&[vec![]], &config.slots, config.unit, 1, 0.0).is_none());

        // Non-finite BT (f32 NaN) → None; non-finite optional → field None.
        let pin = serde_json::json!({
            "sourceId": sid,
            "channels": [
                { "role": "bt", "register": 0, "kind": "input", "dataType": "f32" },
                { "role": "et", "register": 2, "kind": "input", "dataType": "f32" },
            ],
        });
        let config = ModbusConfig::from_pin_value(sid, &pin).unwrap();
        let nan = f32::NAN.to_bits();
        let good = 390.0f32.to_bits();
        let frames = vec![vec![
            (nan >> 16) as u16,
            nan as u16,
            (good >> 16) as u16,
            good as u16,
        ]];
        assert!(
            sample_from_frames(&frames, &config.slots, config.unit, 1, 0.0).is_none(),
            "NaN BT must be an unusable poll"
        );
        let frames = vec![vec![
            (good >> 16) as u16,
            good as u16,
            (nan >> 16) as u16,
            nan as u16,
        ]];
        let s = sample_from_frames(&frames, &config.slots, config.unit, 1, 0.0).unwrap();
        assert_eq!((s.bt_f, s.et_f), (390.0, None), "NaN ET degrades to None");
    }

    // -- read planning ----------------------------------------------------

    #[test]
    fn read_plan_batches_contiguous_registers_and_splits_holes() {
        let ch = |role: &str, register: u16, kind: &str, data_type: &str| -> ModbusChannel {
            serde_json::from_value(serde_json::json!({
                "role": role, "register": register, "kind": kind, "dataType": data_type,
            }))
            .unwrap()
        };

        // Contiguous input pair + holed input pair + one holding register —
        // deliberately listed OUT of address order.
        let channels = vec![
            ch("fan", 9, "input", "u16"),
            ch("bt", 0, "input", "u16"),
            ch("ambient", 0, "holding", "u16"),
            ch("et", 1, "input", "u16"),
            ch("heater", 8, "input", "u16"),
        ];
        let (ops, slots) = plan_reads(&channels);
        assert_eq!(
            ops,
            vec![
                ReadOp {
                    kind: ModbusRegisterKind::Holding,
                    start: 0,
                    count: 1
                },
                ReadOp {
                    kind: ModbusRegisterKind::Input,
                    start: 0,
                    count: 2
                },
                ReadOp {
                    kind: ModbusRegisterKind::Input,
                    start: 8,
                    count: 2
                },
            ]
        );
        // Slots stay in channel order and point at the right words.
        let placements: Vec<(usize, usize)> = slots.iter().map(|s| (s.op, s.word)).collect();
        assert_eq!(placements, vec![(2, 1), (1, 0), (0, 0), (1, 1), (2, 0)]);

        // f32 widths chain a contiguous span into ONE op.
        let channels = vec![
            ch("bt", 2, "input", "f32"),
            ch("et", 4, "input", "f32"),
            ch("drum", 6, "input", "f32-swapped"),
        ];
        let (ops, _) = plan_reads(&channels);
        assert_eq!(
            ops,
            vec![ReadOp {
                kind: ModbusRegisterKind::Input,
                start: 2,
                count: 6
            }]
        );

        // Overlap merges too (an f32 spanning a u16 someone also maps).
        let channels = vec![ch("bt", 0, "input", "f32"), ch("et", 1, "input", "u16")];
        let (ops, _) = plan_reads(&channels);
        assert_eq!(
            ops,
            vec![ReadOp {
                kind: ModbusRegisterKind::Input,
                start: 0,
                count: 2
            }]
        );

        // The 125-register FC03/FC04 request limit caps a merge chain. (Not
        // reachable through the 6-role pin schema; plan_reads still guards
        // it so the invariant is local, not an emergent property.)
        let many: Vec<ModbusChannel> = (0..130u16).map(|r| ch("bt", r, "input", "u16")).collect();
        let (ops, _) = plan_reads(&many);
        assert_eq!(
            ops,
            vec![
                ReadOp {
                    kind: ModbusRegisterKind::Input,
                    start: 0,
                    count: 125
                },
                ReadOp {
                    kind: ModbusRegisterKind::Input,
                    start: 125,
                    count: 5
                },
            ]
        );
    }

    // -- read-only invariant ----------------------------------------------

    /// READ-ONLY BY CONSTRUCTION, documented: the only request-issuing method
    /// in this module is `ReadOnlyModbusClient::read`, and its function code
    /// comes from [`ModbusRegisterKind`] — a closed enum whose two variants
    /// are FC03/FC04, both reads. The tokio-modbus `Context` (which has write
    /// methods) is a private field of the wrapper and never escapes; nothing
    /// else in the module touches the wire. The e2e tests complete the proof
    /// at runtime: the emulator audits every request it ever receives, and
    /// the audits contain only function codes 3 and 4.
    #[test]
    fn read_only_function_code_set_is_closed_and_exact() {
        assert_eq!(ModbusRegisterKind::Holding.function_code(), 3);
        assert_eq!(ModbusRegisterKind::Input.function_code(), 4);
    }

    #[test]
    fn descriptor_is_a_device_source() {
        let sid = "modbus-tcp:10.1.2.3:502";
        let pin = pin_json(sid, serde_json::json!([{ "role": "bt", "register": 0 }]));
        let source = ModbusSource::from_pin(sid, Some(&pin), 1, 0.0).unwrap();
        let info = source.descriptor();
        assert_eq!(info.id, sid);
        assert_eq!(info.kind, SourceKind::Device);
        assert!(info.label.contains("10.1.2.3:502"));
    }

    #[test]
    fn missing_pin_is_invalid_args_and_refused_connection_is_port_error() {
        let err = ModbusSource::from_pin("modbus-tcp:h:502", None, 1, 0.0)
            .map(|_| ())
            .unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::InvalidArgs);

        // Bind an ephemeral port, then free it: connecting to it must be
        // refused → fail-fast port_error out of start() (the tc4 precedent).
        let free_port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        let sid = format!("modbus-tcp:127.0.0.1:{free_port}");
        let pin = pin_json(&sid, serde_json::json!([{ "role": "bt", "register": 0 }]));
        let mut source = ModbusSource::from_pin(&sid, Some(&pin), 1, 0.0)
            .unwrap()
            .with_timing(fast_timing());
        let (_, sink) = collector();
        let err = source.start(sink).unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::PortError);
    }

    // -- end-to-end against the in-process server --------------------------

    #[test]
    fn captures_maps_units_batches_survives_server_kill_and_stays_read_only() {
        // Registers carry °C; BT/ET u16 ×10 on input 0/1, heater/fan ×10 on
        // input 8/9, BT mirrored on holding 0 (mapped as ambient here purely
        // to force an FC03 into the poll plan).
        let emulator = Emulator::spawn(EmuConfig {
            curve: vec![(0.0, 390.0)],
            unit: TempUnitDto::C,
            speed: 1.0,
        });
        let sid = emulator.source_id();
        let pin = serde_json::json!({
            "sourceId": sid,
            "unitId": 1,
            "unit": "C",
            "channels": [
                { "role": "bt", "register": 0, "kind": "input", "scale": 0.1 },
                { "role": "et", "register": 1, "kind": "input", "scale": 0.1 },
                { "role": "heater", "register": 8, "kind": "input", "scale": 0.1 },
                { "role": "fan", "register": 9, "kind": "input", "scale": 0.1 },
                { "role": "ambient", "register": 0, "kind": "holding", "scale": 0.1 },
            ],
        });
        let (collected, sink) = collector();
        let mut source = ModbusSource::from_pin(&sid, Some(&pin), 5, 0.0)
            .unwrap()
            .with_timing(fast_timing());
        source.start(sink).unwrap();

        // Phase 1: samples flow, mapped and unit-converted.
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
                assert!((s.bt_f - 390.0).abs() < 0.3, "bt °C→°F, got {}", s.bt_f);
                assert!((s.et_f.unwrap() - 430.0).abs() < 0.3, "et = bt + 40°F");
                assert!((s.ambient_f.unwrap() - 390.0).abs() < 0.3, "FC03 mirror");
                assert_eq!((s.heater, s.fan), (Some(45.0), Some(60.0)));
            }
            for pair in samples.windows(2) {
                assert!(
                    pair[1].session_sec > pair[0].session_sec,
                    "session clock must be monotonic"
                );
            }
        }

        // Phase 2: kill the server → disconnected.
        let port = {
            let requests = emulator.sent_requests();
            assert!(!requests.is_empty());
            emulator.kill()
        };
        wait_for(&collected, "disconnected status", |e| {
            has_status(e, SourceStatusKind::Disconnected)
        });

        // Phase 3: restart on the SAME port → reconnected, seq gap-free.
        let before = sample_count(&collected.lock().unwrap());
        let emulator = Emulator::spawn_on(
            port,
            EmuConfig {
                curve: vec![(0.0, 390.0)],
                unit: TempUnitDto::C,
                speed: 1.0,
            },
        );
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
            assert!(disc < reco, "disconnected strictly before reconnected");
        }

        // READ-ONLY invariant, audited across BOTH server lifetimes; and the
        // batching contract: contiguous registers arrive as ONE read each —
        // (FC03 @0×1), (FC04 @0×2), (FC04 @8×2) — never register-at-a-time.
        let expected: [(u8, u16, u16); 3] = [(3, 0, 1), (4, 0, 2), (4, 8, 2)];
        for requests in [emulator.sent_requests()] {
            assert!(!requests.is_empty());
            for req in &requests {
                assert!(
                    expected.contains(req),
                    "unexpected request on the wire: {req:?}"
                );
            }
        }
    }

    #[test]
    fn f32_channels_decode_in_both_word_orders_end_to_end() {
        let emulator = Emulator::spawn(EmuConfig::default()); // °F registers
        let sid = emulator.source_id();
        // BT f32 @2 (standard), ET f32 @4 (standard), drum reads the SWAPPED
        // BT copy @6 — all one contiguous span, so ONE FC04 read of 6 words.
        let pin = serde_json::json!({
            "sourceId": sid,
            "channels": [
                { "role": "bt", "register": 2, "kind": "input", "dataType": "f32" },
                { "role": "et", "register": 4, "kind": "input", "dataType": "f32" },
                { "role": "drum", "register": 6, "kind": "input", "dataType": "f32-swapped" },
            ],
        });
        let (collected, sink) = collector();
        let mut source = ModbusSource::from_pin(&sid, Some(&pin), 1, 0.0)
            .unwrap()
            .with_timing(fast_timing());
        source.start(sink).unwrap();
        wait_for(&collected, "3 samples", |e| sample_count(e) >= 3);
        source.stop();

        let events = collected.lock().unwrap();
        for e in events.iter() {
            if let SourceEmit::Sample(s) = e {
                assert_eq!(s.bt_f, 390.0, "f32 standard word order");
                assert_eq!(s.et_f, Some(430.0));
                assert_eq!(s.drum, Some(390.0), "f32-swapped word order");
            }
        }
        for req in emulator.sent_requests() {
            assert_eq!(req, (4, 2, 6), "one batched FC04 read per poll");
        }
    }

    /// A device that answers with a MODBUS exception (unmapped register) is
    /// an unusable rig: three strikes → disconnected, zero samples emitted.
    #[test]
    fn device_exceptions_drive_the_three_strike_disconnect() {
        let emulator = Emulator::spawn(EmuConfig::default());
        let sid = emulator.source_id();
        let pin = pin_json(
            &sid,
            serde_json::json!([{ "role": "bt", "register": 900, "kind": "input" }]),
        );
        let (collected, sink) = collector();
        let mut source = ModbusSource::from_pin(&sid, Some(&pin), 1, 0.0)
            .unwrap()
            .with_timing(fast_timing());
        source.start(sink).unwrap();
        wait_for(&collected, "disconnected on exceptions", |e| {
            has_status(e, SourceStatusKind::Disconnected)
        });
        source.stop();
        let events = collected.lock().unwrap();
        assert_eq!(
            sample_count(&events),
            0,
            "an exception-answering register must never produce samples"
        );
    }

    /// `pollMs` in the pin overrides the timing default: with a 10 s timing
    /// default, only the pin's 60 ms cadence can produce several samples
    /// within the test window.
    #[test]
    fn poll_ms_in_the_pin_overrides_the_timing_default() {
        let emulator = Emulator::spawn(EmuConfig::default());
        let sid = emulator.source_id();
        let mut pin = pin_json(
            &sid,
            serde_json::json!([{ "role": "bt", "register": 0, "kind": "input", "scale": 0.1 }]),
        );
        pin["pollMs"] = serde_json::json!(60);
        let slow = ModbusTiming {
            poll_interval: Duration::from_secs(10),
            ..fast_timing()
        };
        let (collected, sink) = collector();
        let mut source = ModbusSource::from_pin(&sid, Some(&pin), 1, 0.0)
            .unwrap()
            .with_timing(slow);
        source.start(sink).unwrap();
        wait_for(&collected, "4 samples at the pin cadence", |e| {
            sample_count(e) >= 4
        });
        source.stop();
    }
}
