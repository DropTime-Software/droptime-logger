//! capture/ — the capture engine and device sources.
//!
//! Binding architecture rule (build plan §3): capture + persistence live in
//! Rust, and every sample is written to SQLite BEFORE it is emitted on the
//! IPC Channel. The engine owns at most ONE active session; a second
//! `start_session` fails with `session_active`.

pub mod autodetect;
pub mod modbus;
pub mod replay;
pub mod serial;
pub mod tc4;

use std::sync::{Arc, Mutex};

use crate::error::LoggerError;
use crate::model::{
    RoastEventKind, SampleDto, SampleEvent, SourceInfo, SourceKind, SourcePinDto, SourceStatusKind,
    StartSessionArgs,
};
use crate::store::{now_ms, speed_setting_key, ResumeSeed, SampleRow, Store};

use autodetect::AutoMarkDetector;
use modbus::{ModbusSource, ModbusTiming};
use replay::ReplaySource;
use tc4::{Tc4Source, Tc4Timing};

/// Device sources that carry a SourcePin the engine must persist at start and
/// hand back on resume (`tc4:` and `modbus-tcp:`; replay derives everything
/// from the fixture).
fn requires_pin(source_id: &str) -> bool {
    source_id.starts_with("tc4:") || modbus::is_modbus_source_id(source_id)
}

/// `mm:ss` label for gap/outage durations in notes and status messages.
fn format_mm_ss(sec: f64) -> String {
    let total = sec.round().max(0.0) as u64;
    format!("{:02}:{:02}", total / 60, total % 60)
}

/// Wrap `sink` so a Connected/Reconnected status is emitted EXACTLY once,
/// and never before it is due: the returned trigger is called by the engine
/// after `source.start()` succeeds, while the gate also fires from the first
/// source emission (so no sample can ever race ahead of the status). A source
/// that fails to start never emits through its sink, so a failed start leaves
/// no healthy status on the Channel for the UI to reconcile with the error.
fn status_gated_sink(
    inner: SampleSink,
    kind: SourceStatusKind,
    at_session_sec: f64,
) -> (SampleSink, impl Fn()) {
    let once = Arc::new(std::sync::Once::new());
    let fire = {
        let inner = Arc::clone(&inner);
        move || {
            once.call_once(|| {
                inner(SourceEmit::Status {
                    kind,
                    message: None,
                    at_session_sec,
                });
            });
        }
    };
    let gated: SampleSink = {
        let fire = fire.clone();
        Arc::new(move |emit| {
            fire();
            inner(emit);
        })
    };
    (gated, fire)
}

/// Settings key persisting a device session's SourcePin JSON so
/// `resume_session` can reconnect with the same port/register/channel roles
/// (the `speed_setting_key` pattern). Written at start, removed if the start
/// fails.
pub(crate) fn pin_setting_key(roast_uuid: &str) -> String {
    format!("session_pin:{roast_uuid}")
}

// ---------------------------------------------------------------------------
// DeviceSource abstraction
// ---------------------------------------------------------------------------

/// What a source pushes into the engine.
#[derive(Debug, Clone, PartialEq)]
pub enum SourceEmit {
    Sample(SampleDto),
    Status {
        kind: SourceStatusKind,
        message: Option<String>,
        at_session_sec: f64,
    },
}

/// Sink handed to a source's read loop; must be callable from any thread.
pub type SampleSink = Arc<dyn Fn(SourceEmit) + Send + Sync>;

/// Where finished `SampleEvent`s go (the IPC Channel in production, a
/// collector in tests).
pub type Emitter = Box<dyn Fn(SampleEvent) + Send + Sync>;

/// A capture source: replay fixtures in Phase 1, serial/HID/USB drivers in
/// Phase 2a. `start` spawns/owns the read loop; `stop` must join it.
pub trait DeviceSource: Send {
    fn descriptor(&self) -> SourceInfo;
    fn start(&mut self, sink: SampleSink) -> Result<(), LoggerError>;
    fn stop(&mut self);
}

/// Auto CHARGE/DROP feed (CONTRACTS.md §7.2): called with every PERSISTED
/// sample of a device-kind source; returns `Some((kind, sessionSec))` when a
/// mark should fire. Production wraps `AutoMarkDetector`; tests may inject a
/// fake to exercise the wiring.
pub type AutoDetectFn = Box<dyn FnMut(&SampleDto) -> Option<(RoastEventKind, f64)> + Send>;

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

struct ActiveSession {
    roast_uuid: String,
    source: Box<dyn DeviceSource>,
}

/// Owns the (at most one) active capture session and routes
/// source → store → Channel.
pub struct Engine {
    store: Store,
    active: Mutex<Option<ActiveSession>>,
    /// Real-hardware defaults; tests accelerate via `set_tc4_timing`.
    tc4_timing: Tc4Timing,
    /// Real-network defaults; tests accelerate via `set_modbus_timing`.
    modbus_timing: ModbusTiming,
}

impl Engine {
    pub fn new(store: Store) -> Self {
        Self {
            store,
            active: Mutex::new(None),
            tc4_timing: Tc4Timing::default(),
            modbus_timing: ModbusTiming::default(),
        }
    }

    #[cfg(test)]
    pub(crate) fn set_tc4_timing(&mut self, timing: Tc4Timing) {
        self.tc4_timing = timing;
    }

    #[cfg(test)]
    pub(crate) fn set_modbus_timing(&mut self, timing: ModbusTiming) {
        self.modbus_timing = timing;
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn active_roast_uuid(&self) -> Option<String> {
        self.active
            .lock()
            .unwrap()
            .as_ref()
            .map(|s| s.roast_uuid.clone())
    }

    /// Resolve a `sourceId` into a concrete `DeviceSource` (CONTRACTS.md §7.2):
    /// `replay:<fixture>` → ReplaySource; `tc4:<portName>` → Tc4Source
    /// (requires `sourcePin`, else `invalid_args`);
    /// `modbus-tcp:<host>:<port>` → ModbusSource (requires a `sourcePin` with
    /// `channels` mapping a `bt` role, else `invalid_args`). Construction is
    /// side-effect free — only `start()` spawns the read loop.
    /// `start_session_sec` seeds the device session clock (0 for a fresh
    /// session; on resume the wall-anchored seconds since started_wall_ms so
    /// the outage stays on the time axis); replay derives its clock from seq
    /// and ignores it.
    ///
    /// The pin arrives as raw JSON (SourcePin is a driver-defined document,
    /// model.rs): each driver parses and validates its own schema here, so a
    /// malformed pin fails as `invalid_args`.
    fn build_source(
        &self,
        source_id: &str,
        speed: f64,
        start_seq: u64,
        start_session_sec: f64,
        source_pin: Option<&serde_json::Value>,
    ) -> Result<Box<dyn DeviceSource>, LoggerError> {
        if source_id.starts_with("replay:") {
            let fixture = replay::fixture_for_source_id(source_id)
                .ok_or_else(|| LoggerError::source_not_found(source_id))?;
            Ok(Box::new(ReplaySource::new(fixture, speed, start_seq)))
        } else if let Some(port_name) = source_id.strip_prefix("tc4:") {
            let pin = source_pin.ok_or_else(|| {
                LoggerError::invalid_args(
                    "tc4 sources require a sourcePin (port, baud, channel roles)",
                )
            })?;
            let pin: SourcePinDto = serde_json::from_value(pin.clone())
                .map_err(|e| LoggerError::invalid_args(format!("tc4 sourcePin: {e}")))?;
            Ok(Box::new(
                Tc4Source::new(port_name.to_owned(), pin, start_seq, start_session_sec)
                    .with_timing(self.tc4_timing.clone()),
            ))
        } else if modbus::is_modbus_source_id(source_id) {
            Ok(Box::new(
                ModbusSource::from_pin(source_id, source_pin, start_seq, start_session_sec)?
                    .with_timing(self.modbus_timing.clone()),
            ))
        } else {
            Err(LoggerError::source_not_found(source_id))
        }
    }

    /// `autoMark.charge` / `autoMark.drop` settings — 'on' unless set to 'off'.
    fn auto_mark_enabled(&self, key: &str) -> bool {
        match self.store.get_setting(key) {
            Ok(Some(value)) => value != "off",
            _ => true,
        }
    }

    /// Auto CHARGE/DROP detector for DEVICE-kind sources only (§7.2); replay
    /// and simulator sources never auto-mark from the backend.
    ///
    /// On resume (`seed` = the roast's persisted marker state) the detector
    /// is seeded so §7.2 "fires ≤1× per roast per kind" holds across a crash:
    /// a fresh detector would read the DROP plunge of an already-charged
    /// roast as a second CHARGE and corrupt every seconds-from-charge marker.
    fn auto_detector_for(
        &self,
        source: &dyn DeviceSource,
        seed: Option<&ResumeSeed>,
    ) -> Option<AutoDetectFn> {
        if source.descriptor().kind != SourceKind::Device {
            return None;
        }
        let charge = self.auto_mark_enabled("autoMark.charge");
        let drop = self.auto_mark_enabled("autoMark.drop");
        if !charge && !drop {
            return None;
        }
        let mut detector = match seed {
            None => AutoMarkDetector::new(charge, drop),
            Some(seed) => AutoMarkDetector::resumed(
                charge,
                drop,
                // A charge without a pinned temp (no samples near the tap)
                // still consumes the charge stage; 400°F is a plausible
                // stand-in the DROP plausibility gate can work against.
                seed.charge.map(|(sec, temp)| (sec, temp.unwrap_or(400.0))),
                seed.drop_marked,
            ),
        };
        Some(Box::new(move |sample| detector.feed(sample)))
    }

    /// Start a new recording session (CONTRACTS.md §2 `start_session`).
    pub fn start_session(
        &self,
        args: &StartSessionArgs,
        emitter: Emitter,
    ) -> Result<String, LoggerError> {
        let mut active = self.active.lock().unwrap();
        if active.is_some() {
            return Err(LoggerError::session_active());
        }

        let speed = args.speed.unwrap_or(1.0);
        let mut source =
            self.build_source(&args.source_id, speed, 1, 0.0, args.source_pin.as_ref())?;
        let auto_detect = self.auto_detector_for(&*source, None);

        // A running port preview must never hold the port a session needs
        // (§7.2: any start_session implicitly stops the preview).
        serial::stop_any_preview();

        // Shell row + session settings are one logical unit: if ANY of it
        // fails — or the source fails to start — roll ALL of it back, so a
        // half-created 'recording' row can never surface as a bogus
        // pending_recovery on the next launch.
        let roast_uuid = uuid::Uuid::now_v7().to_string();
        let prepared = (|| -> Result<(), LoggerError> {
            self.store
                .create_roast(&roast_uuid, &args.source_id, now_ms(), &args.meta)?;
            // Remembered so resume_session can replay at the same speed.
            self.store
                .set_setting(&speed_setting_key(&roast_uuid), &speed.to_string())?;
            // Device sources: remember the pin so resume_session can
            // reconnect the same rig (tc4 port/channels, modbus register map).
            if requires_pin(&args.source_id) {
                if let Some(pin) = args.source_pin.as_ref() {
                    let json = serde_json::to_string(pin)
                        .map_err(|e| LoggerError::io(format!("sourcePin serialization: {e}")))?;
                    self.store
                        .set_setting(&pin_setting_key(&roast_uuid), &json)?;
                }
            }
            Ok(())
        })();
        if let Err(err) = prepared {
            self.rollback_session_shell(&roast_uuid);
            return Err(err);
        }

        tracing::info!(roast = %roast_uuid, source = %source.descriptor().id, speed, "starting session");

        // `connected` is gated behind a successful start: a failed start must
        // never leave a healthy status on the Channel next to the rejected
        // invoke (the gate also keeps any sample from racing ahead of it).
        let sink = self.make_sink(roast_uuid.clone(), emitter, 0, auto_detect);
        let (sink, mark_connected) = status_gated_sink(sink, SourceStatusKind::Connected, 0.0);
        if let Err(err) = source.start(sink) {
            self.rollback_session_shell(&roast_uuid);
            return Err(err);
        }
        mark_connected();

        *active = Some(ActiveSession {
            roast_uuid: roast_uuid.clone(),
            source,
        });
        Ok(roast_uuid)
    }

    /// Best-effort removal of everything `start_session` had written before
    /// it failed (shell row + speed/pin settings).
    fn rollback_session_shell(&self, roast_uuid: &str) {
        let _ = self.store.delete_roast_shell(roast_uuid);
        let _ = self.store.delete_setting(&speed_setting_key(roast_uuid));
        let _ = self.store.delete_setting(&pin_setting_key(roast_uuid));
    }

    /// Reattach to an orphaned `recording` roast (CONTRACTS.md §2
    /// `resume_session`). Replay continues the fixture from the last
    /// persisted seq; a device (tc4/modbus) reconnects with the stored pin,
    /// re-anchors the session clock to wall time (the outage stays on the
    /// time axis) and records the gap. Emits `status: reconnected` first.
    /// Returns the seq resumed FROM (the last persisted one).
    pub fn resume_session(&self, roast_uuid: &str, emitter: Emitter) -> Result<u64, LoggerError> {
        let mut active = self.active.lock().unwrap();
        if active.is_some() {
            return Err(LoggerError::session_active());
        }

        let (status, source_id) = self
            .store
            .roast_status(roast_uuid)?
            .ok_or_else(|| LoggerError::no_such_roast(roast_uuid))?;
        if status != crate::model::RoastStatus::Recording {
            return Err(LoggerError::new(
                crate::error::ErrorCode::NoSuchRoast,
                format!(
                    "roast {roast_uuid} is not recoverable (status: {})",
                    status.as_str()
                ),
            ));
        }
        // Device resume (tc4/modbus): a live device cannot reproduce past
        // samples, so we reconnect with the stored SourcePin and continue seq
        // from last+1. Replay resumes losslessly (byte-identical), as before.
        let is_device_resume = requires_pin(&source_id);
        let source_pin: Option<serde_json::Value> = if is_device_resume {
            let json = self
                .store
                .get_setting(&pin_setting_key(roast_uuid))?
                .ok_or_else(|| {
                    LoggerError::invalid_args(format!(
                        "no stored sourcePin for device session {roast_uuid}; cannot resume"
                    ))
                })?;
            Some(serde_json::from_str(&json).map_err(|e| {
                LoggerError::invalid_args(format!("stored sourcePin is corrupt: {e}"))
            })?)
        } else {
            None
        };
        let speed = self
            .store
            .get_setting(&speed_setting_key(roast_uuid))?
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(1.0);
        let (last_seq, last_session_sec) = self.store.last_sample(roast_uuid)?.unwrap_or((0, 0.0));
        let seed = self.store.resume_seed(roast_uuid)?;

        // INVARIANT (CONTRACTS.md: sessionSec = seconds since recording
        // started): the physical roast kept going while the app was down, so
        // for a wall-clock DEVICE the resumed clock re-anchors to
        // started_wall_ms — the outage MUST appear on the time axis, or DROP
        // time, DTR and every post-resume marker understate the real roast.
        // Session-clock zero ≈ started_wall_ms (the device anchor is set at
        // thread spawn; boot settle elapses after it), and the .max() guards
        // SystemTime skew so the resumed clock can never regress behind
        // already-persisted samples. Replay keeps last_session_sec: its time
        // axis derives from seq, so a deterministic fixture resumes losslessly.
        let start_session_sec = if is_device_resume {
            (((now_ms() - seed.started_wall_ms) as f64) / 1000.0).max(last_session_sec)
        } else {
            last_session_sec
        };

        // Resuming a tc4 session needs the port back (§7.2 preview rules);
        // harmless for modbus/replay.
        serial::stop_any_preview();

        let mut source = self.build_source(
            &source_id,
            speed,
            last_seq + 1,
            start_session_sec,
            source_pin.as_ref(),
        )?;
        let auto_detect = self.auto_detector_for(&*source, Some(&seed));
        tracing::info!(roast = %roast_uuid, last_seq, speed, "resuming session");

        // `reconnected` is gated exactly like `connected` in start_session:
        // emitted once, only after start() succeeds, ahead of any sample.
        let sink = self.make_sink(roast_uuid.to_owned(), emitter, last_seq, auto_detect);
        let (sink, mark_reconnected) =
            status_gated_sink(sink, SourceStatusKind::Reconnected, start_session_sec);
        source.start(Arc::clone(&sink))?;
        mark_reconnected();

        // Persist the outage for history (store BEFORE emit, §2; sink Status
        // emissions are Channel-only and never stored) — precedent:
        // discard_recovery's gap note. Then tell the live UI where the time
        // axis jumped so it can break the curve instead of interpolating.
        if is_device_resume {
            let outage = format_mm_ss(start_session_sec - last_session_sec);
            let note = format!("gap: app offline {outage} during recording");
            match self
                .store
                .append_note_event(roast_uuid, last_session_sec, &note)
            {
                Ok(()) => sink(SourceEmit::Status {
                    kind: SourceStatusKind::Gap,
                    message: Some(format!("recording gap: {outage} offline")),
                    at_session_sec: start_session_sec,
                }),
                Err(err) => tracing::error!(
                    error = %err, roast = %roast_uuid,
                    "gap note failed to persist; gap status not emitted"
                ),
            }
        }

        *active = Some(ActiveSession {
            roast_uuid: roast_uuid.to_owned(),
            source,
        });
        Ok(last_seq)
    }

    /// Stop capture if `roast_uuid` is the active session (finish/abandon path).
    pub fn stop_if_active(&self, roast_uuid: &str) {
        let mut active = self.active.lock().unwrap();
        if active.as_ref().is_some_and(|s| s.roast_uuid == roast_uuid) {
            if let Some(mut session) = active.take() {
                session.source.stop();
            }
        }
    }

    /// Stop any active capture WITHOUT touching roast status — the app-exit
    /// path. A roast left `recording` here is exactly what `pending_recovery`
    /// finds on next launch. Flushes the open sample batch so every captured
    /// sample is durable before the process exits.
    pub fn shutdown(&self) {
        if let Some(mut session) = self.active.lock().unwrap().take() {
            tracing::info!(roast = %session.roast_uuid, "shutdown with active session; leaving it recoverable");
            session.source.stop();
        }
        if let Err(err) = self.store.flush() {
            tracing::error!(error = %err, "final flush on shutdown failed");
        }
    }

    /// Build the sink: store (blocking, BEFORE emit) → seq-gap detection →
    /// Channel → auto-mark detector. A failed DB write suppresses the UI
    /// emission entirely (and the sample never reaches the detector).
    fn make_sink(
        &self,
        roast_uuid: String,
        emitter: Emitter,
        last_seq: u64,
        auto_detect: Option<AutoDetectFn>,
    ) -> SampleSink {
        let store = self.store.clone();
        let last_seq = Mutex::new(last_seq);
        let auto_detect = Mutex::new(auto_detect);
        Arc::new(move |emit| match emit {
            SourceEmit::Sample(sample) => {
                let row = SampleRow {
                    roast_uuid: roast_uuid.clone(),
                    sample,
                };
                if let Err(err) = store.append_sample(row) {
                    tracing::error!(
                        error = %err, roast = %roast_uuid, seq = sample.seq,
                        "sample failed to persist; NOT emitting to UI"
                    );
                    return;
                }
                let mut last = last_seq.lock().unwrap();
                if sample.seq > *last + 1 {
                    emitter(SampleEvent::Status {
                        kind: SourceStatusKind::Gap,
                        message: Some(format!(
                            "sample gap: missing seq {}–{}",
                            *last + 1,
                            sample.seq - 1
                        )),
                        at_session_sec: sample.session_sec,
                    });
                }
                *last = sample.seq;
                emitter(sample.to_event());

                // Auto CHARGE/DROP (§7.2): fed AFTER the sample is persisted
                // and visible; a firing detector marks via the same store path
                // as `mark_event`, then emits the §6 `marker` event.
                let fired = auto_detect
                    .lock()
                    .unwrap()
                    .as_mut()
                    .and_then(|detect| detect(&sample));
                if let Some((kind, at_session_sec)) = fired {
                    // Defense in depth: an auto mark must never RE-tap a kind
                    // that is already marked (auto or manual) — the latest tap
                    // wins canonicalization, so a re-tap would silently move
                    // the operator's marker. Manual re-taps stay untouched.
                    match store.has_event(&roast_uuid, kind) {
                        Ok(false) => {}
                        Ok(true) => {
                            tracing::warn!(
                                roast = %roast_uuid, kind = kind.as_str(), at_session_sec,
                                "auto-mark suppressed: kind already marked"
                            );
                            return;
                        }
                        Err(err) => {
                            tracing::error!(
                                error = %err, roast = %roast_uuid, kind = kind.as_str(),
                                "auto-mark exists-check failed; marker not applied"
                            );
                            return;
                        }
                    }
                    match store.mark_event(
                        &roast_uuid,
                        kind,
                        Some(at_session_sec),
                        Some("auto".into()),
                    ) {
                        Ok(markers) => {
                            tracing::info!(
                                roast = %roast_uuid, kind = kind.as_str(), at_session_sec,
                                "auto-mark fired"
                            );
                            emitter(SampleEvent::Marker {
                                kind,
                                auto: true,
                                at_session_sec,
                                markers,
                            });
                        }
                        Err(err) => tracing::error!(
                            error = %err, roast = %roast_uuid, kind = kind.as_str(),
                            "auto-mark failed to persist; marker not emitted"
                        ),
                    }
                }
            }
            SourceEmit::Status {
                kind,
                message,
                at_session_sec,
            } => {
                emitter(SampleEvent::Status {
                    kind,
                    message,
                    at_session_sec,
                });
            }
        })
    }
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{RoastEventKind, RoastStatus, SessionMetaDto};
    use crate::store::test_db_path;
    use std::time::{Duration, Instant};

    type Collected = Arc<Mutex<Vec<SampleEvent>>>;

    fn collector() -> (Collected, Emitter) {
        let collected: Collected = Arc::new(Mutex::new(Vec::new()));
        let emitter: Emitter = {
            let collected = Arc::clone(&collected);
            Box::new(move |ev| collected.lock().unwrap().push(ev))
        };
        (collected, emitter)
    }

    fn wait_for_samples(collected: &Collected, n: usize) {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let count = collected
                .lock()
                .unwrap()
                .iter()
                .filter(|e| matches!(e, SampleEvent::Sample { .. }))
                .count();
            if count >= n {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {n} samples (got {count})"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn start_args(speed: f64) -> StartSessionArgs {
        StartSessionArgs {
            source_id: "replay:fast-decaf".into(),
            speed: Some(speed),
            meta: SessionMetaDto {
                coffee_name: Some("Sumatra Decaf".into()),
                charge_weight_lb: Some(20.0),
                ..Default::default()
            },
            source_pin: None,
        }
    }

    #[test]
    fn engine_routes_store_before_channel_and_finishes() {
        // start_session stops any running preview (process-global slot); hold
        // the preview test lock so a parallel preview test isn't cut short.
        let _preview_guard = serial::preview_test_lock();
        let store = Store::open(&test_db_path("engine")).unwrap();
        let engine = Engine::new(store);
        let (collected, emitter) = collector();

        let uuid = engine.start_session(&start_args(400.0), emitter).unwrap();
        assert_eq!(engine.active_roast_uuid().as_deref(), Some(uuid.as_str()));

        // Second session while one is recording → session_active.
        let (_, e2) = collector();
        let err = engine.start_session(&start_args(400.0), e2).unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::SessionActive);

        // Capture well past the fixture's 76s turning point, then stop.
        wait_for_samples(&collected, 120);
        engine.stop_if_active(&uuid);
        assert!(engine.active_roast_uuid().is_none());

        // First event is status connected; samples are contiguous from seq 1.
        {
            let events = collected.lock().unwrap();
            assert!(matches!(
                events[0],
                SampleEvent::Status {
                    kind: SourceStatusKind::Connected,
                    ..
                }
            ));
            let seqs: Vec<u64> = events
                .iter()
                .filter_map(|e| match e {
                    SampleEvent::Sample { seq, .. } => Some(*seq),
                    _ => None,
                })
                .collect();
            assert_eq!(seqs[0], 1);
            assert!(
                seqs.windows(2).all(|w| w[1] == w[0] + 1),
                "no seq gaps expected"
            );
        }

        // Every emitted sample must already be persisted (store-before-channel).
        let emitted = collected
            .lock()
            .unwrap()
            .iter()
            .filter(|e| matches!(e, SampleEvent::Sample { .. }))
            .count();
        let persisted = engine.store().get_roast(&uuid).unwrap().samples.len();
        assert!(
            persisted >= emitted,
            "persisted ({persisted}) must never trail emitted ({emitted})"
        );

        // Unknown source id → source_not_found (and no stray roast row).
        let (_, e3) = collector();
        let bad = StartSessionArgs {
            source_id: "replay:missing".into(),
            speed: None,
            meta: SessionMetaDto::default(),
            source_pin: None,
        };
        let err = engine.start_session(&bad, e3).unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::SourceNotFound);
        assert_eq!(engine.store().list_roasts().unwrap().len(), 1);

        // Mark charge at t=0 and finish → auto turning point (parity semantics).
        engine
            .store()
            .mark_event(&uuid, RoastEventKind::Charge, Some(0.0), None)
            .unwrap();
        let summary = engine
            .store()
            .finish_roast(&uuid, Some(17.4), None)
            .unwrap();
        assert_eq!(summary.status, RoastStatus::Finished);
        let tp = summary
            .markers
            .turning_point_sec
            .expect("turning point auto-detected");
        // fast-decaf fixture TP is at 76s (bt min 176); on the 1s-interpolated
        // curve the detected minimum must land on the same knot.
        assert!(
            (tp - 76.0).abs() <= 3.0,
            "tp {tp} too far from fixture's 76s"
        );
    }

    #[test]
    fn crash_recovery_resume_continues_seq_with_reconnected_status() {
        let _preview_guard = serial::preview_test_lock();
        let path = test_db_path("engine-recovery");
        let store = Store::open(&path).unwrap();
        let uuid;
        {
            // "Process 1": record some samples, then die without finishing.
            let engine = Engine::new(store.clone());
            let (collected, emitter) = collector();
            uuid = engine.start_session(&start_args(400.0), emitter).unwrap();
            wait_for_samples(&collected, 30);
            engine.shutdown(); // stops capture, leaves status = recording
        }
        store.flush().unwrap();

        // "Process 2": a fresh engine sees the orphan.
        let engine = Engine::new(store.clone());
        let rec = engine
            .store()
            .pending_recovery(engine.active_roast_uuid())
            .unwrap()
            .expect("orphaned recording roast must surface");
        assert_eq!(rec.roast_uuid, uuid);
        assert!(rec.last_seq >= 30);
        assert_eq!(rec.last_session_sec, (rec.last_seq - 1) as f64);
        assert_eq!(rec.meta.coffee_name.as_deref(), Some("Sumatra Decaf"));

        // Resume: reconnected status first, then samples from last_seq + 1.
        let (collected, emitter) = collector();
        let resumed_from = engine.resume_session(&uuid, emitter).unwrap();
        assert_eq!(resumed_from, rec.last_seq);
        wait_for_samples(&collected, 5);
        engine.stop_if_active(&uuid);
        {
            let events = collected.lock().unwrap();
            match &events[0] {
                SampleEvent::Status {
                    kind,
                    at_session_sec,
                    ..
                } => {
                    assert_eq!(*kind, SourceStatusKind::Reconnected);
                    assert_eq!(*at_session_sec, rec.last_session_sec);
                }
                other => panic!("first resumed event must be reconnected, got {other:?}"),
            }
            let first_seq = events
                .iter()
                .find_map(|e| match e {
                    SampleEvent::Sample { seq, .. } => Some(*seq),
                    _ => None,
                })
                .unwrap();
            assert_eq!(
                first_seq,
                rec.last_seq + 1,
                "resume must continue the fixture"
            );
        }

        // The persisted curve is gap-free across the crash boundary.
        store.flush().unwrap();
        let roast = store.get_roast(&uuid).unwrap();
        let max_seq = roast.samples.iter().map(|s| s.seq).max().unwrap();
        assert_eq!(
            roast.samples.len() as u64,
            max_seq,
            "seq must be contiguous 1..=max"
        );

        // Resuming a finished roast is refused.
        store.finish_roast(&uuid, None, None).unwrap();
        let (_, emitter) = collector();
        let err = engine.resume_session(&uuid, emitter).unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::NoSuchRoast);
    }

    #[test]
    fn tc4_source_resolution_requires_pin_then_fails_fast_on_a_missing_port() {
        use crate::model::{SourcePinDto, TempUnitDto};

        let _preview_guard = serial::preview_test_lock();
        let engine = Engine::new(Store::open(&test_db_path("tc4-resolve")).unwrap());

        // tc4:<port> without a sourcePin → invalid_args (CONTRACTS §7.2).
        let (_, e1) = collector();
        let args = StartSessionArgs {
            source_id: "tc4:/dev/definitely-not-a-port".into(),
            speed: None,
            meta: SessionMetaDto::default(),
            source_pin: None,
        };
        let err = engine.start_session(&args, e1).unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::InvalidArgs);

        // With a pin the factory resolves Tc4Source; opening a nonexistent
        // port fails fast with port_error and the roast shell is rolled back
        // (including the persisted pin).
        let (_, e2) = collector();
        let args = StartSessionArgs {
            source_pin: Some(
                serde_json::to_value(SourcePinDto {
                    source_id: "tc4:/dev/definitely-not-a-port".into(),
                    baud: 115_200,
                    bt_channel: 1,
                    et_channel: Some(2),
                    unit: TempUnitDto::F,
                })
                .unwrap(),
            ),
            ..args
        };
        let err = engine.start_session(&args, e2).unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::PortError);
        assert!(engine.active_roast_uuid().is_none());
        assert_eq!(
            engine.store().list_roasts().unwrap().len(),
            0,
            "failed start must roll back"
        );
        let leaked = engine
            .store()
            .get_setting(&pin_setting_key("nonexistent"))
            .unwrap();
        assert!(leaked.is_none());
    }

    /// The §7.2 definition of done: a full simulated roast captured
    /// end-to-end through the REAL engine off a pty — TC4 driver → store →
    /// events — with auto CHARGE and DROP firing, then a clean finish.
    #[cfg(unix)]
    #[test]
    fn tc4_end_to_end_roast_auto_marks_charge_and_drop_through_the_engine() {
        use crate::model::{SourcePinDto, TempUnitDto};
        use tc4::emu::{EmuConfig, Emulator};

        let _preview_guard = serial::preview_test_lock();
        // A compressed roast in wall-clock seconds: hot soak → charge plunge
        // → recovery/development → drop plunge → cool.
        let emulator = Emulator::spawn(EmuConfig {
            curve: vec![
                (0.0, 390.0),
                (0.8, 390.0),  // preheat soak
                (1.6, 170.0),  // charge plunge to the turning point
                (3.2, 415.0),  // development ramp
                (4.0, 250.0),  // drop plunge
                (30.0, 250.0), // cool hold
            ],
            bt_slot: 1,
            et_slot: 2,
            power_fields: true,
        });

        let store = Store::open(&test_db_path("tc4-e2e")).unwrap();
        let mut engine = Engine::new(store);
        engine.set_tc4_timing(tc4::Tc4Timing {
            boot_settle: Duration::from_millis(30),
            poll_interval: Duration::from_millis(20),
            chunk_timeout: Duration::from_millis(10),
            response_deadline: Duration::from_millis(80),
            ack_deadline: Duration::from_millis(40),
            backoff_min: Duration::from_millis(40),
            backoff_max: Duration::from_millis(160),
        });

        let (collected, emitter) = collector();
        let args = StartSessionArgs {
            source_id: format!("tc4:{}", emulator.slave_path),
            speed: None,
            meta: SessionMetaDto {
                coffee_name: Some("PTY Guji".into()),
                charge_weight_lb: Some(12.0),
                ..Default::default()
            },
            source_pin: Some(
                serde_json::to_value(SourcePinDto {
                    source_id: format!("tc4:{}", emulator.slave_path),
                    baud: 115_200,
                    bt_channel: 1,
                    et_channel: Some(2),
                    unit: TempUnitDto::F,
                })
                .unwrap(),
            ),
        };
        let uuid = engine.start_session(&args, emitter).unwrap();

        // Wait for both auto markers to land on the stream.
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let markers: Vec<(RoastEventKind, bool)> = collected
                .lock()
                .unwrap()
                .iter()
                .filter_map(|e| match e {
                    SampleEvent::Marker { kind, auto, .. } => Some((*kind, *auto)),
                    _ => None,
                })
                .collect();
            if markers.len() >= 2 {
                assert_eq!(
                    markers,
                    vec![(RoastEventKind::Charge, true), (RoastEventKind::Drop, true)],
                    "exactly auto CHARGE then auto DROP"
                );
                break;
            }
            assert!(
                Instant::now() < deadline,
                "auto markers never fired: {markers:?}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        engine.stop_if_active(&uuid);

        // Canonical markers persisted via the same path as mark_event.
        let roast = engine.store().get_roast(&uuid).unwrap();
        let charge_temp = roast
            .summary
            .markers
            .charge_temp_f
            .expect("charge temp set");
        assert!(
            (charge_temp - 390.0).abs() < 15.0,
            "charge fired near the soak temp, got {charge_temp}"
        );
        let drop_temp = roast.summary.markers.drop_temp_f.expect("drop temp set");
        assert!(
            drop_temp > 380.0 && drop_temp <= 416.0,
            "drop fired near the development peak, got {drop_temp}"
        );
        let drop_sec = roast.summary.markers.drop_sec.expect("drop sec set");
        assert!(drop_sec > 0.0, "drop is after charge on the roast clock");
        for kind in [RoastEventKind::Charge, RoastEventKind::Drop] {
            let ev = roast
                .events
                .iter()
                .find(|e| e.kind == kind)
                .expect("event row");
            assert_eq!(ev.note.as_deref(), Some("auto"));
        }

        // And the roast finishes cleanly.
        let summary = engine
            .store()
            .finish_roast(&uuid, Some(10.4), None)
            .unwrap();
        assert_eq!(summary.status, RoastStatus::Finished);

        // READ-ONLY invariant through the whole engine path.
        for cmd in emulator.sent_commands() {
            let verb = cmd.split(';').next().unwrap().to_string();
            assert!(
                matches!(verb.as_str(), "CHAN" | "UNITS" | "FILT" | "READ"),
                "non-read-only command on the wire: {cmd}"
            );
        }
    }

    /// tc4 resume: reconnect with the stored source_pin, seq continues from
    /// last+1, and the session clock re-anchors to WALL time — the outage
    /// while the app was down must appear in session_sec (regression: the
    /// clock used to continue from the last persisted second, erasing the
    /// outage from the time axis). The replay resume path stays untouched
    /// (its test above still runs).
    #[cfg(unix)]
    #[test]
    fn tc4_resume_reconnects_with_stored_pin_and_continues_seq_and_clock() {
        use crate::model::{SourcePinDto, TempUnitDto};
        use tc4::emu::{EmuConfig, Emulator};

        let _preview_guard = serial::preview_test_lock();
        let emulator = Emulator::spawn(EmuConfig::default());
        let fast = tc4::Tc4Timing {
            boot_settle: Duration::from_millis(30),
            poll_interval: Duration::from_millis(20),
            chunk_timeout: Duration::from_millis(10),
            response_deadline: Duration::from_millis(80),
            ack_deadline: Duration::from_millis(40),
            backoff_min: Duration::from_millis(40),
            backoff_max: Duration::from_millis(160),
        };
        let path = test_db_path("tc4-resume");
        let store = Store::open(&path).unwrap();
        let args = StartSessionArgs {
            source_id: format!("tc4:{}", emulator.slave_path),
            speed: None,
            meta: SessionMetaDto::default(),
            source_pin: Some(
                serde_json::to_value(SourcePinDto {
                    source_id: format!("tc4:{}", emulator.slave_path),
                    baud: 115_200,
                    bt_channel: 1,
                    et_channel: Some(2),
                    unit: TempUnitDto::F,
                })
                .unwrap(),
            ),
        };

        let uuid;
        {
            // "Process 1": capture some samples, then die without finishing.
            let mut engine = Engine::new(store.clone());
            engine.set_tc4_timing(fast.clone());
            let (collected, emitter) = collector();
            uuid = engine.start_session(&args, emitter).unwrap();
            wait_for_samples(&collected, 5);
            engine.shutdown();
        }
        store.flush().unwrap();

        // The app stays dead for a measurable outage — the physical roast
        // keeps going, so this time MUST reappear on the session clock.
        let outage = Duration::from_millis(300);
        std::thread::sleep(outage);

        // "Process 2": the orphan surfaces and resumes onto the SAME rig.
        let mut engine = Engine::new(store.clone());
        engine.set_tc4_timing(fast);
        let rec = engine
            .store()
            .pending_recovery(engine.active_roast_uuid())
            .unwrap()
            .expect("orphaned tc4 roast must surface");
        assert_eq!(rec.roast_uuid, uuid);
        assert!(rec.last_seq >= 5);

        let (collected, emitter) = collector();
        let wall_before_resume_ms = crate::store::now_ms();
        let resumed_from = engine.resume_session(&uuid, emitter).unwrap();
        assert_eq!(resumed_from, rec.last_seq);
        wait_for_samples(&collected, 3);
        engine.stop_if_active(&uuid);

        {
            let events = collected.lock().unwrap();
            match &events[0] {
                SampleEvent::Status { kind, .. } => {
                    assert_eq!(*kind, SourceStatusKind::Reconnected);
                }
                other => panic!("first resumed event must be reconnected, got {other:?}"),
            }
            // The outage is announced to the live UI right after reconnected.
            assert!(
                events.iter().any(|e| matches!(
                    e,
                    SampleEvent::Status {
                        kind: SourceStatusKind::Gap,
                        ..
                    }
                )),
                "a gap status must follow a device resume"
            );
            let first = events
                .iter()
                .find_map(|e| match e {
                    SampleEvent::Sample {
                        seq, session_sec, ..
                    } => Some((*seq, *session_sec)),
                    _ => None,
                })
                .unwrap();
            assert_eq!(
                first.0,
                rec.last_seq + 1,
                "seq continues from last persisted + 1"
            );
            // Clock honesty: sessionSec = seconds since recording started, so
            // the first resumed sample carries the FULL wall elapsed since
            // started_wall_ms — including the outage — not just
            // last_session_sec + a poll interval.
            let wall_elapsed_sec = (wall_before_resume_ms - rec.started_wall_ms) as f64 / 1000.0;
            assert!(
                first.1 >= wall_elapsed_sec - 0.05,
                "resumed clock must include the outage: got {} < wall elapsed {}",
                first.1,
                wall_elapsed_sec
            );
            assert!(
                first.1 >= rec.last_session_sec + outage.as_secs_f64() - 0.05,
                "resumed clock erased the {}s outage (got {} after {})",
                outage.as_secs_f64(),
                first.1,
                rec.last_session_sec
            );
        }

        // The persisted curve is seq-gap-free across the crash boundary, and
        // the outage is recorded for history as a gap note at the last
        // pre-crash second (sink statuses are Channel-only, never stored).
        store.flush().unwrap();
        let roast = store.get_roast(&uuid).unwrap();
        let mut seqs: Vec<u64> = roast.samples.iter().map(|s| s.seq).collect();
        seqs.sort_unstable();
        assert!(
            seqs.windows(2).all(|w| w[1] == w[0] + 1),
            "seq contiguous across resume"
        );
        assert_eq!(seqs.first(), Some(&1));
        let note = roast
            .events
            .iter()
            .find(|e| e.kind == RoastEventKind::Note)
            .expect("resume must persist a gap note event");
        assert!(note.note.as_deref().unwrap_or("").contains("gap"));
        assert_eq!(note.session_sec, rec.last_session_sec);
    }

    /// Regression (resume-no-re-arm): a roast that CHARGEd before the crash
    /// resumes with the auto-mark detector SEEDED from the persisted markers.
    /// When the operator then drops the beans, the hot BT plunge — which is
    /// exactly the CHARGE signature to a fresh detector — must NOT fire a
    /// second charge (§7.2: auto-mark fires ≤1× per roast per KIND, across
    /// process boundaries), or every seconds-from-charge marker re-derives
    /// against the bogus origin.
    #[cfg(unix)]
    #[test]
    fn tc4_resume_never_rereads_the_drop_plunge_as_a_second_charge() {
        use crate::model::{SourcePinDto, TempUnitDto};
        use tc4::emu::{EmuConfig, Emulator};

        let _preview_guard = serial::preview_test_lock();
        // Wall-clock curve: a long hot soak, then the bean-dump plunge well
        // after the resume completes (≤ ~1 s in), then a cool hold.
        let emulator = Emulator::spawn(EmuConfig {
            curve: vec![
                (0.0, 400.0),
                (4.0, 400.0),  // hot soak across crash + resume
                (5.5, 170.0),  // DROP plunge — the charge-shaped signature
                (30.0, 170.0), // cool hold
            ],
            bt_slot: 1,
            et_slot: 2,
            power_fields: true,
        });
        let fast = tc4::Tc4Timing {
            boot_settle: Duration::from_millis(30),
            poll_interval: Duration::from_millis(20),
            chunk_timeout: Duration::from_millis(10),
            response_deadline: Duration::from_millis(80),
            ack_deadline: Duration::from_millis(40),
            backoff_min: Duration::from_millis(40),
            backoff_max: Duration::from_millis(160),
        };
        let path = test_db_path("tc4-resume-rearm");
        let store = Store::open(&path).unwrap();
        let args = StartSessionArgs {
            source_id: format!("tc4:{}", emulator.slave_path),
            speed: None,
            meta: SessionMetaDto::default(),
            source_pin: Some(
                serde_json::to_value(SourcePinDto {
                    source_id: format!("tc4:{}", emulator.slave_path),
                    baud: 115_200,
                    bt_channel: 1,
                    et_channel: Some(2),
                    unit: TempUnitDto::F,
                })
                .unwrap(),
            ),
        };

        let uuid;
        let charge_before_crash;
        {
            // "Process 1": capture, CHARGE gets marked, then the app dies.
            let mut engine = Engine::new(store.clone());
            engine.set_tc4_timing(fast.clone());
            let (collected, emitter) = collector();
            uuid = engine.start_session(&args, emitter).unwrap();
            wait_for_samples(&collected, 5);
            let markers = engine
                .store()
                .mark_event(&uuid, RoastEventKind::Charge, None, None)
                .unwrap();
            charge_before_crash = markers.charge_temp_f;
            assert!(charge_before_crash.is_some(), "charge pinned pre-crash");
            engine.shutdown();
        }
        store.flush().unwrap();
        let charge_sec_before: Option<f64> = {
            let seed = store.resume_seed(&uuid).unwrap();
            seed.charge.map(|(sec, _)| sec)
        };
        assert!(
            charge_sec_before.is_some(),
            "charge persisted before the crash"
        );

        // "Process 2": resume, then ride through the drop plunge.
        let mut engine = Engine::new(store.clone());
        engine.set_tc4_timing(fast);
        let (collected, emitter) = collector();
        engine.resume_session(&uuid, emitter).unwrap();
        // Wait until the plunge has fully played out on the resumed stream.
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let plunged = collected
                .lock()
                .unwrap()
                .iter()
                .any(|e| matches!(e, SampleEvent::Sample { bt_f, .. } if *bt_f < 200.0));
            if plunged {
                break;
            }
            assert!(Instant::now() < deadline, "emulator plunge never arrived");
            std::thread::sleep(Duration::from_millis(10));
        }
        engine.stop_if_active(&uuid);
        store.flush().unwrap();

        // Exactly ONE charge row — the pre-crash one — and the canonical
        // charge origin did not move to the plunge.
        let roast = store.get_roast(&uuid).unwrap();
        let charges: Vec<_> = roast
            .events
            .iter()
            .filter(|e| e.kind == RoastEventKind::Charge)
            .collect();
        assert_eq!(charges.len(), 1, "resume re-fired charge: {charges:?}");
        let seed = store.resume_seed(&uuid).unwrap();
        assert_eq!(
            seed.charge.map(|(sec, _)| sec),
            charge_sec_before,
            "charge_session_sec moved after resume"
        );
        // And no auto-marker event reached the stream after resume.
        let stream_markers: Vec<_> = collected
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                SampleEvent::Marker { kind, .. } => Some(*kind),
                _ => None,
            })
            .collect();
        assert!(
            stream_markers.is_empty(),
            "no auto marker may fire on the plunge, got {stream_markers:?}"
        );
    }

    #[test]
    fn auto_mark_wiring_marks_store_and_emits_marker_event() {
        // The DETECTOR is stubbed (returns None), but the engine wiring —
        // persisted sample → detector → store mark → §6 Marker emission — must
        // be real. Exercise it with a fake detector through the actual sink.
        let store = Store::open(&test_db_path("auto-mark")).unwrap();
        let engine = Engine::new(store);
        let uuid = "roast-auto";
        engine
            .store()
            .create_roast(uuid, "tc4:usbserial-FAKE", 1, &SessionMetaDto::default())
            .unwrap();

        let (collected, emitter) = collector();
        // Fake detector: fires CHARGE exactly once, when BT crosses 300°F.
        let mut fired = false;
        let detect: AutoDetectFn = Box::new(move |s| {
            if !fired && s.bt_f >= 300.0 {
                fired = true;
                Some((RoastEventKind::Charge, s.session_sec))
            } else {
                None
            }
        });
        let sink = engine.make_sink(uuid.to_string(), emitter, 0, Some(detect));

        for seq in 1..=5u64 {
            sink(SourceEmit::Sample(SampleDto {
                seq,
                session_sec: (seq - 1) as f64,
                bt_f: 280.0 + 10.0 * seq as f64, // crosses 300 at seq 2 (t = 1.0)
                et_f: None,
                ambient_f: None,
                heater: None,
                fan: None,
                drum: None,
            }));
        }

        let events = collected.lock().unwrap();
        let markers: Vec<(RoastEventKind, bool, f64)> = events
            .iter()
            .filter_map(|e| match e {
                SampleEvent::Marker {
                    kind,
                    auto,
                    at_session_sec,
                    ..
                } => Some((*kind, *auto, *at_session_sec)),
                _ => None,
            })
            .collect();
        assert_eq!(
            markers,
            vec![(RoastEventKind::Charge, true, 1.0)],
            "exactly one auto marker event, auto: true, at the crossing sample"
        );
        // The marker event must follow its triggering sample on the stream.
        let sample2_idx = events
            .iter()
            .position(|e| matches!(e, SampleEvent::Sample { seq: 2, .. }))
            .expect("sample 2 emitted");
        let marker_idx = events
            .iter()
            .position(|e| matches!(e, SampleEvent::Marker { .. }))
            .expect("marker emitted");
        assert!(
            marker_idx > sample2_idx,
            "marker must be emitted after its sample"
        );
        drop(events);

        // Same store path as mark_event: raw events row + canonical markers.
        let roast = engine.store().get_roast(uuid).unwrap();
        let charge = roast
            .events
            .iter()
            .find(|e| e.kind == RoastEventKind::Charge)
            .expect("charge event row appended");
        assert_eq!(charge.session_sec, 1.0);
        assert_eq!(charge.note.as_deref(), Some("auto"));
        assert_eq!(roast.summary.markers.charge_temp_f, Some(300.0));
    }

    // -- modbus (engine-level; the driver-level suite lives in modbus.rs) --

    fn modbus_pin(source_id: &str) -> serde_json::Value {
        serde_json::json!({
            "sourceId": source_id,
            "unitId": 1,
            "unit": "C",
            "channels": [
                { "role": "bt", "register": 0, "kind": "input", "scale": 0.1 },
                { "role": "et", "register": 1, "kind": "input", "scale": 0.1 },
            ],
        })
    }

    #[test]
    fn modbus_source_resolution_requires_pin_with_bt_role_then_fails_fast() {
        let _preview_guard = serial::preview_test_lock();
        let engine = Engine::new(Store::open(&test_db_path("modbus-resolve")).unwrap());

        // modbus-tcp:<host>:<port> without a sourcePin → invalid_args (§7.2).
        let (_, e1) = collector();
        let args = StartSessionArgs {
            source_id: "modbus-tcp:127.0.0.1:1502".into(),
            speed: None,
            meta: SessionMetaDto::default(),
            source_pin: None,
        };
        let err = engine.start_session(&args, e1).unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::InvalidArgs);

        // A pin whose channels lack a bt role → invalid_args too.
        let (_, e2) = collector();
        let no_bt = StartSessionArgs {
            source_pin: Some(serde_json::json!({
                "sourceId": "modbus-tcp:127.0.0.1:1502",
                "channels": [{ "role": "et", "register": 1 }],
            })),
            ..args.clone()
        };
        let err = engine.start_session(&no_bt, e2).unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::InvalidArgs);

        // With a valid pin the factory resolves ModbusSource; connecting to a
        // port nobody listens on fails fast with port_error and the roast
        // shell is rolled back (including the persisted pin).
        let free_port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        let source_id = format!("modbus-tcp:127.0.0.1:{free_port}");
        let (_, e3) = collector();
        let refused = StartSessionArgs {
            source_id: source_id.clone(),
            source_pin: Some(modbus_pin(&source_id)),
            ..args
        };
        let err = engine.start_session(&refused, e3).unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::PortError);
        assert!(engine.active_roast_uuid().is_none());
        assert_eq!(
            engine.store().list_roasts().unwrap().len(),
            0,
            "failed start must roll back"
        );
    }

    /// The modbus twin of the tc4 definition of done: a full simulated roast
    /// captured end-to-end through the REAL engine off an in-process
    /// MODBUS-TCP server — driver → store → events — with auto CHARGE and
    /// DROP firing, then a clean finish. Platform-independent (no pty).
    #[test]
    fn modbus_end_to_end_roast_auto_marks_charge_and_drop_through_the_engine() {
        use crate::model::TempUnitDto;
        use modbus::emu::{fast_timing, EmuConfig, Emulator};

        let _preview_guard = serial::preview_test_lock();
        // A compressed roast in wall-clock seconds: hot soak → charge plunge
        // → recovery/development → drop plunge → cool. Registers carry °C so
        // the full unit-conversion path is exercised.
        let emulator = Emulator::spawn(EmuConfig {
            curve: vec![
                (0.0, 390.0),
                (0.8, 390.0),  // preheat soak
                (1.6, 170.0),  // charge plunge to the turning point
                (3.2, 415.0),  // development ramp
                (4.0, 250.0),  // drop plunge
                (30.0, 250.0), // cool hold
            ],
            unit: TempUnitDto::C,
            speed: 1.0,
        });

        let store = Store::open(&test_db_path("modbus-e2e")).unwrap();
        let mut engine = Engine::new(store);
        engine.set_modbus_timing(fast_timing());

        let source_id = emulator.source_id();
        let (collected, emitter) = collector();
        let args = StartSessionArgs {
            source_id: source_id.clone(),
            speed: None,
            meta: SessionMetaDto {
                coffee_name: Some("MODBUS Guji".into()),
                charge_weight_lb: Some(12.0),
                ..Default::default()
            },
            source_pin: Some(modbus_pin(&source_id)),
        };
        let uuid = engine.start_session(&args, emitter).unwrap();

        // Wait for both auto markers to land on the stream.
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let markers: Vec<(RoastEventKind, bool)> = collected
                .lock()
                .unwrap()
                .iter()
                .filter_map(|e| match e {
                    SampleEvent::Marker { kind, auto, .. } => Some((*kind, *auto)),
                    _ => None,
                })
                .collect();
            if markers.len() >= 2 {
                assert_eq!(
                    markers,
                    vec![(RoastEventKind::Charge, true), (RoastEventKind::Drop, true)],
                    "exactly auto CHARGE then auto DROP"
                );
                break;
            }
            assert!(
                Instant::now() < deadline,
                "auto markers never fired: {markers:?}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        engine.stop_if_active(&uuid);

        // Canonical markers persisted via the same path as mark_event.
        let roast = engine.store().get_roast(&uuid).unwrap();
        let charge_temp = roast
            .summary
            .markers
            .charge_temp_f
            .expect("charge temp set");
        assert!(
            (charge_temp - 390.0).abs() < 15.0,
            "charge fired near the soak temp, got {charge_temp}"
        );
        let drop_temp = roast.summary.markers.drop_temp_f.expect("drop temp set");
        assert!(
            drop_temp > 380.0 && drop_temp <= 416.0,
            "drop fired near the development peak, got {drop_temp}"
        );
        let drop_sec = roast.summary.markers.drop_sec.expect("drop sec set");
        assert!(drop_sec > 0.0, "drop is after charge on the roast clock");
        for kind in [RoastEventKind::Charge, RoastEventKind::Drop] {
            let ev = roast
                .events
                .iter()
                .find(|e| e.kind == kind)
                .expect("event row");
            assert_eq!(ev.note.as_deref(), Some("auto"));
        }

        // Store-before-emit: every emitted sample is already persisted.
        let emitted = collected
            .lock()
            .unwrap()
            .iter()
            .filter(|e| matches!(e, SampleEvent::Sample { .. }))
            .count();
        assert!(
            roast.samples.len() >= emitted,
            "persisted ({}) must never trail emitted ({emitted})",
            roast.samples.len()
        );

        // And the roast finishes cleanly.
        let summary = engine
            .store()
            .finish_roast(&uuid, Some(10.4), None)
            .unwrap();
        assert_eq!(summary.status, RoastStatus::Finished);

        // READ-ONLY invariant through the whole engine path: the server
        // audited every request ever received — only FC03/FC04 may appear.
        let requests = emulator.sent_requests();
        assert!(!requests.is_empty());
        for (fc, _, _) in requests {
            assert!(
                fc == 3 || fc == 4,
                "non-read function code on the wire: FC{fc:02}"
            );
        }
    }

    /// modbus resume: reconnect with the stored source_pin, seq continues
    /// from last+1, and the session clock re-anchors to WALL time — the
    /// outage while the app was down must appear in session_sec (the same
    /// honest-clock semantics the tc4 resume test pins down).
    #[test]
    fn modbus_resume_reconnects_with_stored_pin_and_continues_seq_and_clock() {
        use modbus::emu::{fast_timing, EmuConfig, Emulator};

        let _preview_guard = serial::preview_test_lock();
        // The rig (and the physical roast) outlives the app "crash".
        let emulator = Emulator::spawn(EmuConfig {
            curve: vec![(0.0, 390.0)],
            unit: crate::model::TempUnitDto::C,
            speed: 1.0,
        });
        let source_id = emulator.source_id();
        let path = test_db_path("modbus-resume");
        let store = Store::open(&path).unwrap();
        let args = StartSessionArgs {
            source_id: source_id.clone(),
            speed: None,
            meta: SessionMetaDto::default(),
            source_pin: Some(modbus_pin(&source_id)),
        };

        let uuid;
        {
            // "Process 1": capture some samples, then die without finishing.
            let mut engine = Engine::new(store.clone());
            engine.set_modbus_timing(fast_timing());
            let (collected, emitter) = collector();
            uuid = engine.start_session(&args, emitter).unwrap();
            wait_for_samples(&collected, 5);
            engine.shutdown();
        }
        store.flush().unwrap();

        // The app stays dead for a measurable outage — the physical roast
        // keeps going, so this time MUST reappear on the session clock.
        let outage = Duration::from_millis(300);
        std::thread::sleep(outage);

        // "Process 2": the orphan surfaces and resumes onto the SAME rig.
        let mut engine = Engine::new(store.clone());
        engine.set_modbus_timing(fast_timing());
        let rec = engine
            .store()
            .pending_recovery(engine.active_roast_uuid())
            .unwrap()
            .expect("orphaned modbus roast must surface");
        assert_eq!(rec.roast_uuid, uuid);
        assert!(rec.last_seq >= 5);

        let (collected, emitter) = collector();
        let wall_before_resume_ms = crate::store::now_ms();
        let resumed_from = engine.resume_session(&uuid, emitter).unwrap();
        assert_eq!(resumed_from, rec.last_seq);
        wait_for_samples(&collected, 3);
        engine.stop_if_active(&uuid);

        {
            let events = collected.lock().unwrap();
            match &events[0] {
                SampleEvent::Status { kind, .. } => {
                    assert_eq!(*kind, SourceStatusKind::Reconnected);
                }
                other => panic!("first resumed event must be reconnected, got {other:?}"),
            }
            // The outage is announced to the live UI right after reconnected.
            assert!(
                events.iter().any(|e| matches!(
                    e,
                    SampleEvent::Status {
                        kind: SourceStatusKind::Gap,
                        ..
                    }
                )),
                "a gap status must follow a device resume"
            );
            let first = events
                .iter()
                .find_map(|e| match e {
                    SampleEvent::Sample {
                        seq, session_sec, ..
                    } => Some((*seq, *session_sec)),
                    _ => None,
                })
                .unwrap();
            assert_eq!(
                first.0,
                rec.last_seq + 1,
                "seq continues from last persisted + 1"
            );
            // Clock honesty: the first resumed sample carries the FULL wall
            // elapsed since started_wall_ms — including the outage.
            let wall_elapsed_sec = (wall_before_resume_ms - rec.started_wall_ms) as f64 / 1000.0;
            assert!(
                first.1 >= wall_elapsed_sec - 0.05,
                "resumed clock must include the outage: got {} < wall elapsed {}",
                first.1,
                wall_elapsed_sec
            );
            assert!(
                first.1 >= rec.last_session_sec + outage.as_secs_f64() - 0.05,
                "resumed clock erased the {}s outage (got {} after {})",
                outage.as_secs_f64(),
                first.1,
                rec.last_session_sec
            );
        }

        // The persisted curve is seq-gap-free across the crash boundary, and
        // the outage is recorded for history as a gap note at the last
        // pre-crash second.
        store.flush().unwrap();
        let roast = store.get_roast(&uuid).unwrap();
        let mut seqs: Vec<u64> = roast.samples.iter().map(|s| s.seq).collect();
        seqs.sort_unstable();
        assert!(
            seqs.windows(2).all(|w| w[1] == w[0] + 1),
            "seq contiguous across resume"
        );
        assert_eq!(seqs.first(), Some(&1));
        let note = roast
            .events
            .iter()
            .find(|e| e.kind == RoastEventKind::Note)
            .expect("resume must persist a gap note event");
        assert!(note.note.as_deref().unwrap_or("").contains("gap"));
        assert_eq!(note.session_sec, rec.last_session_sec);
    }
}
