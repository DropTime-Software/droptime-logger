//! ReplaySource — driver #0. Replays a bundled `RoastFixture` (the SAME JSON
//! files the TS SimulatorSource replays; CONTRACTS.md §3) through the full
//! native path: capture engine → SQLite → Channel.
//!
//! Semantics:
//!   * fixture curve t is seconds-from-charge; replay maps it to sessionSec
//!     starting at 0 from the first curve point (fixtures start at t = 0);
//!   * the coarse fixture (5s knots) is linearly interpolated to a 1s cadence;
//!   * emitted values are a pure function of (fixture, seq) — timing jitter
//!     never changes the data, which is what makes replay deterministic;
//!   * the timing thread is drift-corrected: each emission waits for an
//!     ABSOLUTE deadline (`start + n·interval/speed`), so sleep overshoot
//!     doesn't accumulate;
//!   * seq starts at 1; resume continues from `start_seq = last persisted + 1`.

use std::sync::mpsc;
use std::sync::OnceLock;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::error::LoggerError;
use crate::model::{SampleDto, SourceInfo, SourceKind, SourceStatusKind};

use super::{DeviceSource, SampleSink, SourceEmit};

/// Replay emits at a fixed 1s roast-time cadence (contract: "interpolated to
/// 1s cadence"), scaled by the speed multiplier in wall time.
pub const EMIT_INTERVAL_SEC: f64 = 1.0;

const MIN_SPEED: f64 = 0.05;
const MAX_SPEED: f64 = 1000.0;

// ---------------------------------------------------------------------------
// Fixtures (embedded at compile time; shape = RoastFixture in types.ts)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FixturePoint {
    pub t: f64,
    pub bt: f64,
    #[serde(default)]
    pub et: Option<f64>,
}

// Contract shape: fields mirror `RoastFixture.markers` in types.ts even where
// the Rust replay driver doesn't read them (the TS SimulatorSource does).
#[allow(dead_code)]
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FixtureMarkers {
    #[serde(default)]
    pub charge_temp_f: Option<f64>,
    #[serde(default)]
    pub turning_point_sec: Option<f64>,
    #[serde(default)]
    pub turning_point_temp_f: Option<f64>,
    #[serde(default)]
    pub dry_end_sec: Option<f64>,
    #[serde(default)]
    pub fc_start_sec: Option<f64>,
    #[serde(default)]
    pub fc_end_sec: Option<f64>,
    #[serde(default)]
    pub drop_sec: Option<f64>,
    #[serde(default)]
    pub drop_temp_f: Option<f64>,
    #[serde(default)]
    pub charge_weight_lb: Option<f64>,
    #[serde(default)]
    pub coffee_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoastFixture {
    pub name: String,
    #[serde(default)]
    #[allow(dead_code)] // contract field (types.ts RoastFixture)
    pub description: Option<String>,
    /// Original sampling interval of the recorded data; replay interpolates to
    /// `EMIT_INTERVAL_SEC` regardless.
    #[allow(dead_code)]
    pub sample_interval_sec: f64,
    pub curve: Vec<FixturePoint>,
    #[serde(default)]
    pub markers: FixtureMarkers,
}

impl RoastFixture {
    fn t0(&self) -> f64 {
        self.curve.first().map(|p| p.t).unwrap_or(0.0)
    }

    /// Number of 1s-cadence samples the full replay emits (seq 1..=count).
    pub fn emit_count(&self, interval_sec: f64) -> u64 {
        match (self.curve.first(), self.curve.last()) {
            (Some(first), Some(last)) if last.t > first.t => {
                ((last.t - first.t) / interval_sec).floor() as u64 + 1
            }
            (Some(_), Some(_)) => 1,
            _ => 0,
        }
    }

    /// Linear interpolation of (bt, et) at `session_sec` seconds after the
    /// first curve point. None outside the curve span.
    pub fn value_at(&self, session_sec: f64) -> Option<(f64, Option<f64>)> {
        let target = self.t0() + session_sec;
        let last = self.curve.last()?;
        if target < self.t0() || target > last.t {
            return None;
        }
        // First index with t >= target.
        let idx = self.curve.partition_point(|p| p.t < target);
        let hi = self.curve.get(idx)?;
        if (hi.t - target).abs() < f64::EPSILON || idx == 0 {
            return Some((hi.bt, hi.et));
        }
        let lo = &self.curve[idx - 1];
        let span = hi.t - lo.t;
        if span <= 0.0 {
            return Some((hi.bt, hi.et));
        }
        let frac = (target - lo.t) / span;
        let bt = lo.bt + (hi.bt - lo.bt) * frac;
        let et = match (lo.et, hi.et) {
            (Some(a), Some(b)) => Some(a + (b - a) * frac),
            _ => None,
        };
        Some((bt, et))
    }

    pub fn source_id(&self) -> String {
        format!("replay:{}", self.name)
    }

    pub fn source_info(&self) -> SourceInfo {
        SourceInfo {
            id: self.source_id(),
            label: self
                .markers
                .coffee_name
                .clone()
                .unwrap_or_else(|| self.name.clone()),
            kind: SourceKind::Replay,
        }
    }
}

/// The three baseline fixtures (CONTRACTS.md §3), embedded so `list_sources`
/// is dependency-free in Phase 1.
static FIXTURES_RAW: &[&str] = &[
    include_str!("../../../../../packages/roast-console/fixtures/ethiopia-guji.json"),
    include_str!("../../../../../packages/roast-console/fixtures/colombia-stall.json"),
    include_str!("../../../../../packages/roast-console/fixtures/fast-decaf.json"),
];

pub fn fixtures() -> &'static [RoastFixture] {
    static PARSED: OnceLock<Vec<RoastFixture>> = OnceLock::new();
    PARSED.get_or_init(|| {
        FIXTURES_RAW
            .iter()
            .map(|raw| {
                serde_json::from_str::<RoastFixture>(raw)
                    .expect("bundled roast fixture must parse (validated by cargo test)")
            })
            .collect()
    })
}

pub fn list_replay_sources() -> Vec<SourceInfo> {
    fixtures().iter().map(RoastFixture::source_info).collect()
}

/// Resolve `replay:<fixtureName>` → fixture.
pub fn fixture_for_source_id(source_id: &str) -> Option<&'static RoastFixture> {
    let name = source_id.strip_prefix("replay:")?;
    fixtures().iter().find(|f| f.name == name)
}

// ---------------------------------------------------------------------------
// ReplaySource
// ---------------------------------------------------------------------------

pub struct ReplaySource {
    fixture: &'static RoastFixture,
    speed: f64,
    start_seq: u64,
    worker: Option<(mpsc::SyncSender<()>, JoinHandle<()>)>,
}

impl ReplaySource {
    /// `start_seq` is the first seq to EMIT (1 for a fresh session,
    /// `last_persisted + 1` when resuming).
    pub fn new(fixture: &'static RoastFixture, speed: f64, start_seq: u64) -> Self {
        let speed = if speed.is_finite() {
            speed.clamp(MIN_SPEED, MAX_SPEED)
        } else {
            1.0
        };
        Self {
            fixture,
            speed,
            start_seq: start_seq.max(1),
            worker: None,
        }
    }
}

impl DeviceSource for ReplaySource {
    fn descriptor(&self) -> SourceInfo {
        self.fixture.source_info()
    }

    fn start(&mut self, sink: SampleSink) -> Result<(), LoggerError> {
        if self.worker.is_some() {
            return Err(LoggerError::io("replay source already started"));
        }
        let (stop_tx, stop_rx) = mpsc::sync_channel::<()>(1);
        let fixture = self.fixture;
        let speed = self.speed;
        let start_seq = self.start_seq;
        let handle = std::thread::Builder::new()
            .name(format!("replay-{}", fixture.name))
            .spawn(move || run_replay(fixture, speed, start_seq, &stop_rx, &sink))
            .map_err(|e| LoggerError::io(format!("failed to spawn replay thread: {e}")))?;
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

impl Drop for ReplaySource {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run_replay(
    fixture: &'static RoastFixture,
    speed: f64,
    start_seq: u64,
    stop_rx: &mpsc::Receiver<()>,
    sink: &SampleSink,
) {
    let interval = EMIT_INTERVAL_SEC;
    let total = fixture.emit_count(interval);
    let session_sec_of = |seq: u64| (seq - 1) as f64 * interval;

    if start_seq > total {
        // Resuming past the end of the fixture — nothing left to replay.
        let at = if total == 0 {
            0.0
        } else {
            session_sec_of(total)
        };
        sink(SourceEmit::Status {
            kind: SourceStatusKind::Ended,
            message: Some("replay fixture already exhausted".into()),
            at_session_sec: at,
        });
        return;
    }

    let started = Instant::now();
    let mut last_session_sec = session_sec_of(start_seq);

    for seq in start_seq..=total {
        // Drift-corrected wait: absolute deadline per seq, interruptible by stop().
        let deadline =
            started + Duration::from_secs_f64((seq - start_seq) as f64 * interval / speed);
        loop {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            match stop_rx.recv_timeout(deadline - now) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }

        let session_sec = session_sec_of(seq);
        let Some((bt_f, et_f)) = fixture.value_at(session_sec) else {
            break;
        };
        sink(SourceEmit::Sample(SampleDto {
            seq,
            session_sec,
            bt_f,
            et_f,
            ambient_f: None,
            heater: None,
            fan: None,
            drum: None,
        }));
        last_session_sec = session_sec;
    }

    sink(SourceEmit::Status {
        kind: SourceStatusKind::Ended,
        message: Some(format!("replay of {} finished", fixture.name)),
        at_session_sec: last_session_sec,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn bundled_fixtures_parse_and_match_contract_shape() {
        let all = fixtures();
        assert_eq!(all.len(), 3);
        let names: Vec<&str> = all.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["ethiopia-guji", "colombia-stall", "fast-decaf"]);
        for f in all {
            assert!(f.sample_interval_sec > 0.0);
            assert!(f.curve.len() > 10, "{} curve too short", f.name);
            assert!(
                f.curve.windows(2).all(|w| w[1].t > w[0].t),
                "{} t not increasing",
                f.name
            );
            assert!(f.markers.drop_sec.is_some(), "{} missing dropSec", f.name);
        }
    }

    #[test]
    fn list_sources_shape() {
        let sources = list_replay_sources();
        assert_eq!(sources.len(), 3);
        for s in &sources {
            assert!(
                s.id.starts_with("replay:"),
                "id must be replay:<fixtureName>"
            );
            assert_eq!(s.kind, SourceKind::Replay);
        }
        assert!(fixture_for_source_id("replay:ethiopia-guji").is_some());
        assert!(fixture_for_source_id("replay:nope").is_none());
        assert!(
            fixture_for_source_id("ethiopia-guji").is_none(),
            "prefix is required"
        );
    }

    #[test]
    fn interpolates_between_fixture_knots() {
        let f = fixture_for_source_id("replay:ethiopia-guji").unwrap();
        // Knots: t=0 bt=388, t=5 bt=374.9 → t=1 is 388 + (374.9-388)/5
        let (bt, et) = f.value_at(1.0).unwrap();
        assert!((bt - (388.0 + (374.9 - 388.0) / 5.0)).abs() < 1e-9);
        assert!(et.is_some());
        // Exact knot passes through untouched.
        let (bt0, _) = f.value_at(0.0).unwrap();
        assert!((bt0 - 388.0).abs() < 1e-12);
        // Past the end → None
        assert!(f.value_at(1e6).is_none());
    }

    fn collect_replay(speed: f64, start_seq: u64, take: usize) -> Vec<SampleDto> {
        let fixture = fixture_for_source_id("replay:fast-decaf").unwrap();
        let collected: Arc<Mutex<Vec<SampleDto>>> = Arc::new(Mutex::new(Vec::new()));
        let ended = Arc::new(Mutex::new(false));
        let sink: SampleSink = {
            let collected = Arc::clone(&collected);
            let ended = Arc::clone(&ended);
            Arc::new(move |emit| match emit {
                SourceEmit::Sample(s) => collected.lock().unwrap().push(s),
                SourceEmit::Status {
                    kind: SourceStatusKind::Ended,
                    ..
                } => {
                    *ended.lock().unwrap() = true;
                }
                SourceEmit::Status { .. } => {}
            })
        };
        let mut source = ReplaySource::new(fixture, speed, start_seq);
        source.start(sink).unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if collected.lock().unwrap().len() >= take || *ended.lock().unwrap() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "replay produced too few samples in time"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        source.stop();
        let mut v = collected.lock().unwrap().clone();
        v.truncate(take);
        v
    }

    #[test]
    fn replay_is_deterministic_at_20x() {
        let a = collect_replay(20.0, 1, 24);
        let b = collect_replay(20.0, 1, 24);
        assert_eq!(a.len(), 24);
        assert_eq!(a, b, "two 20x replays must emit byte-identical samples");

        // 1s cadence, seq from 1, values = fixture interpolation.
        let fixture = fixture_for_source_id("replay:fast-decaf").unwrap();
        for (i, s) in a.iter().enumerate() {
            assert_eq!(s.seq, i as u64 + 1);
            assert!((s.session_sec - i as f64).abs() < 1e-12);
            let (bt, et) = fixture.value_at(s.session_sec).unwrap();
            assert_eq!(s.bt_f, bt);
            assert_eq!(s.et_f, et);
        }
    }

    #[test]
    fn resume_continues_from_seq() {
        let resumed = collect_replay(20.0, 101, 5);
        assert_eq!(resumed[0].seq, 101);
        assert!((resumed[0].session_sec - 100.0).abs() < 1e-12);
        let fixture = fixture_for_source_id("replay:fast-decaf").unwrap();
        let (bt, _) = fixture.value_at(100.0).unwrap();
        assert_eq!(resumed[0].bt_f, bt);
    }

    #[test]
    fn resume_past_end_emits_ended_only() {
        let fixture = fixture_for_source_id("replay:fast-decaf").unwrap();
        let total = fixture.emit_count(EMIT_INTERVAL_SEC);
        let events: Arc<Mutex<Vec<SourceEmit>>> = Arc::new(Mutex::new(Vec::new()));
        let sink: SampleSink = {
            let events = Arc::clone(&events);
            Arc::new(move |e| events.lock().unwrap().push(e))
        };
        let mut source = ReplaySource::new(fixture, 20.0, total + 1);
        source.start(sink).unwrap();
        source.stop(); // join
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            SourceEmit::Status { kind, .. } => assert_eq!(*kind, SourceStatusKind::Ended),
            other => panic!("expected ended status, got {other:?}"),
        }
    }
}
