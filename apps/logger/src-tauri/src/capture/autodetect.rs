//! capture/autodetect.rs — first-principles auto CHARGE/DROP detection from
//! the BT signature (CONTRACTS.md §7.2).
//!
//! Owner: driver. The ENGINE owns the wiring (capture/mod.rs): for
//! `device`-kind sources it feeds every persisted sample to this detector;
//! when it fires, the engine marks via the same store path as `mark_event`
//! and emits the §6 `marker` stream event (`auto: true`). Per-kind enablement
//! comes from the settings keys `autoMark.charge` / `autoMark.drop`
//! ('on' default, 'off' disables). Fires at most once per roast per kind.
//!
//! Detection is first-principles, from the BT curve alone — no protocol or
//! third-party heuristics involved:
//!
//! * **CHARGE** — a hot probe (≥ ~250°F) goes into sustained steep decline
//!   when room-temperature beans hit the drum: smoothed RoR ≤ −2°F/s for ≥3
//!   consecutive samples. Fires at the decline START (the recent BT peak kept
//!   in a ring buffer), not at the confirmation sample.
//! * **DROP** — only after a charge signature, and only once the curve has
//!   recovered past the turning point (BT ≥ post-charge minimum + 15°F).
//!   A sustained sharp decline (smoothed RoR ≤ −1.5°F/s for ≥4 consecutive
//!   samples) whose peak is development-plausible — ≥300 s after charge OR
//!   peak BT ≥ 380°F — fires at the decline start.
//!
//! RoR is a least-squares slope over the last few samples, which keeps the
//! detector robust against per-sample sensor noise (validated against the
//! bundled fixtures with σ≈1.5°F gaussian noise; see tests). The hot path is
//! allocation-free after construction: one fixed-capacity ring buffer, a
//! 5-point regression per sample.

use std::collections::VecDeque;

use crate::model::{RoastEventKind, SampleDto};

/// Samples in the least-squares RoR window.
const SLOPE_WINDOW: usize = 5;
/// Ring-buffer capacity (seconds of history at the 1 Hz cadence).
const BUF_CAP: usize = 24;
/// How far back to look for the decline-start peak when a run begins.
const PEAK_LOOKBACK: usize = SLOPE_WINDOW + 2;

/// CHARGE: smoothed RoR at or below this…
const CHARGE_SLOPE_F_PER_S: f64 = -2.0;
/// …for this many consecutive samples…
const CHARGE_RUN: usize = 3;
/// …starting from a peak at least this hot.
const CHARGE_MIN_BT_F: f64 = 250.0;

/// DROP: smoothed RoR at or below this…
const DROP_SLOPE_F_PER_S: f64 = -1.5;
/// …for this many consecutive samples.
const DROP_RUN: usize = 4;
/// Development plausibility: this long after charge OR…
const DROP_MIN_ELAPSED_SEC: f64 = 300.0;
/// …the decline peak at least this hot (fast roasts).
const DROP_MIN_BT_F: f64 = 380.0;
/// BT must recover this far above the post-charge minimum (turning point)
/// before DROP arms — the post-charge plunge can never read as a drop.
const RECOVERY_RISE_F: f64 = 15.0;

pub struct AutoMarkDetector {
    charge_enabled: bool,
    drop_enabled: bool,
    fired_charge: bool,
    fired_drop: bool,
    /// (session_sec, bt_f) ring buffer, oldest first.
    buf: VecDeque<(f64, f64)>,
    /// Charge signature seen (session_sec, bt) — tracked even when the charge
    /// mark itself is disabled, because DROP is gated on it.
    charge_at: Option<(f64, f64)>,
    /// Drop signature confirmed (fires the mark at most once regardless).
    drop_done: bool,
    post_charge_min_bt: f64,
    drop_armed: bool,
    /// Consecutive samples meeting the current phase's decline threshold.
    run: usize,
    /// Decline-start estimate captured when the current run began.
    candidate: Option<(f64, f64)>,
}

impl AutoMarkDetector {
    pub fn new(charge_enabled: bool, drop_enabled: bool) -> Self {
        Self {
            charge_enabled,
            drop_enabled,
            fired_charge: false,
            fired_drop: false,
            buf: VecDeque::with_capacity(BUF_CAP),
            charge_at: None,
            drop_done: false,
            post_charge_min_bt: f64::INFINITY,
            drop_armed: false,
            run: 0,
            candidate: None,
        }
    }

    /// Detector for a RESUMED session (crash recovery), seeded from the
    /// markers already persisted for the roast so "fires ≤1× per roast per
    /// kind" holds across process boundaries — a fresh detector would treat
    /// the DROP plunge of an already-charged roast as a second CHARGE.
    ///
    /// * `charge_at`: the persisted charge `(session_sec, bt_f)` if one is
    ///   marked (auto or manual). Seeding it consumes the charge stage AND
    ///   gates DROP exactly like a live-detected charge.
    /// * `drop_marked`: a drop tap exists — the detector is fully disarmed.
    ///
    /// `post_charge_min_bt` deliberately keeps the fresh-detector behavior
    /// (first resumed sample becomes the minimum; DROP arms only after a
    /// +15°F recovery above it). Worst case auto-DROP stays disarmed and the
    /// operator taps manually — it can never corrupt a marker.
    pub fn resumed(
        charge_enabled: bool,
        drop_enabled: bool,
        charge_at: Option<(f64, f64)>,
        drop_marked: bool,
    ) -> Self {
        let mut detector = Self::new(charge_enabled, drop_enabled);
        if let Some(seed) = charge_at {
            detector.charge_at = Some(seed);
            detector.fired_charge = true;
        }
        if drop_marked {
            detector.drop_done = true;
            detector.fired_drop = true;
        }
        detector
    }

    /// Feed one persisted sample. Returns `Some((kind, sessionSec))` when a
    /// mark should fire — at most once per kind per roast; the engine performs
    /// the mark + marker emission.
    pub fn feed(&mut self, sample: &SampleDto) -> Option<(RoastEventKind, f64)> {
        if self.buf.len() == BUF_CAP {
            self.buf.pop_front();
        }
        self.buf.push_back((sample.session_sec, sample.bt_f));
        if self.buf.len() < SLOPE_WINDOW {
            return None;
        }
        let slope = self.recent_slope()?;
        if self.charge_at.is_none() {
            self.feed_charge(slope)
        } else {
            self.feed_drop(slope, sample.bt_f)
        }
    }

    fn feed_charge(&mut self, slope: f64) -> Option<(RoastEventKind, f64)> {
        if slope > CHARGE_SLOPE_F_PER_S {
            self.run = 0;
            self.candidate = None;
            return None;
        }
        if self.run == 0 {
            self.candidate = Some(self.recent_peak());
        }
        self.run += 1;
        if self.run < CHARGE_RUN {
            return None;
        }
        let (peak_sec, peak_bt) = self.candidate.take().unwrap_or_else(|| self.recent_peak());
        self.run = 0;
        if peak_bt < CHARGE_MIN_BT_F {
            // A cool decline (warm-up fiddling, empty drum) is not a charge.
            return None;
        }
        self.charge_at = Some((peak_sec, peak_bt));
        self.post_charge_min_bt = self.buf.back().map(|&(_, bt)| bt).unwrap_or(peak_bt);
        if self.charge_enabled && !self.fired_charge {
            self.fired_charge = true;
            return Some((RoastEventKind::Charge, peak_sec));
        }
        None
    }

    fn feed_drop(&mut self, slope: f64, bt: f64) -> Option<(RoastEventKind, f64)> {
        if self.drop_done {
            return None;
        }
        if bt < self.post_charge_min_bt {
            self.post_charge_min_bt = bt;
        }
        if !self.drop_armed {
            if bt >= self.post_charge_min_bt + RECOVERY_RISE_F {
                self.drop_armed = true;
                self.run = 0;
                self.candidate = None;
            }
            return None;
        }
        if slope > DROP_SLOPE_F_PER_S {
            self.run = 0;
            self.candidate = None;
            return None;
        }
        if self.run == 0 {
            self.candidate = Some(self.recent_peak());
        }
        self.run += 1;
        if self.run < DROP_RUN {
            return None;
        }
        let (peak_sec, peak_bt) = self.candidate.take().unwrap_or_else(|| self.recent_peak());
        self.run = 0;
        let charge_sec = self.charge_at.map(|(sec, _)| sec).unwrap_or(0.0);
        let plausible = peak_sec - charge_sec >= DROP_MIN_ELAPSED_SEC || peak_bt >= DROP_MIN_BT_F;
        if !plausible {
            return None;
        }
        self.drop_done = true;
        if self.drop_enabled && !self.fired_drop {
            self.fired_drop = true;
            return Some((RoastEventKind::Drop, peak_sec));
        }
        None
    }

    /// Least-squares slope (°F/s) over the last `SLOPE_WINDOW` samples, using
    /// real timestamps so irregular cadence and gaps stay well-behaved.
    fn recent_slope(&self) -> Option<f64> {
        let n = SLOPE_WINDOW.min(self.buf.len());
        let start = self.buf.len() - n;
        let mut mean_t = 0.0;
        let mut mean_y = 0.0;
        for &(t, y) in self.buf.iter().skip(start) {
            mean_t += t;
            mean_y += y;
        }
        mean_t /= n as f64;
        mean_y /= n as f64;
        let mut num = 0.0;
        let mut den = 0.0;
        for &(t, y) in self.buf.iter().skip(start) {
            num += (t - mean_t) * (y - mean_y);
            den += (t - mean_t) * (t - mean_t);
        }
        if den <= f64::EPSILON {
            return None;
        }
        Some(num / den)
    }

    /// The latest maximum-BT sample within the last `PEAK_LOOKBACK` samples —
    /// the decline-start estimate ("return the earlier session_sec").
    fn recent_peak(&self) -> (f64, f64) {
        let n = PEAK_LOOKBACK.min(self.buf.len());
        let start = self.buf.len() - n;
        let mut best = (0.0, f64::NEG_INFINITY);
        for &(t, y) in self.buf.iter().skip(start) {
            if y >= best.1 {
                best = (t, y);
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::super::replay::{fixture_for_source_id, fixtures, RoastFixture};
    use super::*;

    /// Deterministic gaussian noise: xorshift64* + Box–Muller. No external
    /// crates; identical sequences on every run and platform.
    struct DetRng(u64);

    impl DetRng {
        fn new(seed: u64) -> Self {
            Self(seed.max(1))
        }

        fn next_u64(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            x.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }

        /// Uniform in (0, 1].
        fn uniform(&mut self) -> f64 {
            (((self.next_u64() >> 11) as f64) + 1.0) / ((1u64 << 53) as f64)
        }

        fn gaussian(&mut self, sigma: f64) -> f64 {
            let u1 = self.uniform();
            let u2 = self.uniform();
            sigma * (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
        }
    }

    const PREHEAT_SEC: usize = 90;
    const COOLDOWN_SEC: usize = 60;

    /// Build a realistic full session at 1 s cadence: a hot quasi-stable
    /// preheat soak, the fixture curve (which starts at charge), then the
    /// post-drop cool-down the live engine deliberately keeps capturing.
    /// Ground truth: charge at `PREHEAT_SEC`, drop at `PREHEAT_SEC + dropSec`.
    fn synth_session(fixture: &RoastFixture) -> Vec<(f64, f64)> {
        let charge_bt = fixture.curve.first().expect("fixture curve").bt;
        let last_t = fixture.curve.last().expect("fixture curve").t;
        let mut out = Vec::new();
        for t in 0..PREHEAT_SEC {
            out.push((t as f64, charge_bt));
        }
        let mut t_fix = 0.0;
        let mut last_bt = charge_bt;
        while t_fix <= last_t {
            let (bt, _) = fixture.value_at(t_fix).expect("inside curve span");
            out.push((PREHEAT_SEC as f64 + t_fix, bt));
            last_bt = bt;
            t_fix += 1.0;
        }
        let drop_session = PREHEAT_SEC as f64 + last_t;
        for k in 1..=COOLDOWN_SEC {
            out.push((drop_session + k as f64, last_bt - 3.0 * k as f64));
        }
        out
    }

    fn feed_all(
        detector: &mut AutoMarkDetector,
        session: &[(f64, f64)],
        noise: Option<&mut DetRng>,
    ) -> Vec<(RoastEventKind, f64)> {
        let mut rng = noise;
        let mut fired = Vec::new();
        for &(t, bt) in session {
            let bt = bt
                + match rng {
                    Some(ref mut r) => r.gaussian(1.5),
                    None => 0.0,
                };
            let sample = SampleDto {
                seq: (t as u64) + 1,
                session_sec: t,
                bt_f: bt,
                et_f: None,
                ambient_f: None,
                heater: None,
                fan: None,
                drum: None,
            };
            if let Some(hit) = detector.feed(&sample) {
                fired.push(hit);
            }
        }
        fired
    }

    #[test]
    fn all_three_fixtures_auto_mark_within_ten_seconds_with_noise() {
        for (i, fixture) in fixtures().iter().enumerate() {
            let session = synth_session(fixture);
            let true_charge = PREHEAT_SEC as f64;
            let true_drop = true_charge + fixture.curve.last().unwrap().t;

            for seed_ix in 0..8u64 {
                let mut rng = DetRng::new(0xD40F_71ED ^ ((i as u64 + 1) << 17) ^ (seed_ix << 40));
                let mut detector = AutoMarkDetector::new(true, true);
                let fired = feed_all(&mut detector, &session, Some(&mut rng));

                assert_eq!(
                    fired.len(),
                    2,
                    "{} (seed {seed_ix}): expected exactly charge + drop, got {fired:?}",
                    fixture.name
                );
                let (charge_kind, charge_sec) = fired[0];
                let (drop_kind, drop_sec) = fired[1];
                assert_eq!(charge_kind, RoastEventKind::Charge, "{}", fixture.name);
                assert_eq!(drop_kind, RoastEventKind::Drop, "{}", fixture.name);
                assert!(
                    (charge_sec - true_charge).abs() <= 10.0,
                    "{} (seed {seed_ix}): charge fired at {charge_sec}, ground truth {true_charge}",
                    fixture.name
                );
                assert!(
                    (drop_sec - true_drop).abs() <= 10.0,
                    "{} (seed {seed_ix}): drop fired at {drop_sec}, ground truth {true_drop}",
                    fixture.name
                );
                assert!(
                    charge_sec < drop_sec,
                    "{}: charge must precede drop",
                    fixture.name
                );
            }
        }
    }

    #[test]
    fn raw_fixture_without_preheat_still_charges_near_zero() {
        // Live capture may begin mid-preheat or right at charge; the detector
        // must not require a long stable soak before the decline.
        let fixture = fixture_for_source_id("replay:ethiopia-guji").unwrap();
        let last_t = fixture.curve.last().unwrap().t;
        let mut session = Vec::new();
        let mut t = 0.0;
        let mut last_bt = 0.0;
        while t <= last_t {
            let (bt, _) = fixture.value_at(t).unwrap();
            session.push((t, bt));
            last_bt = bt;
            t += 1.0;
        }
        for k in 1..=COOLDOWN_SEC {
            session.push((last_t + k as f64, last_bt - 3.0 * k as f64));
        }

        let mut detector = AutoMarkDetector::new(true, true);
        let fired = feed_all(&mut detector, &session, None);
        assert_eq!(fired.len(), 2, "expected charge + drop, got {fired:?}");
        assert_eq!(fired[0].0, RoastEventKind::Charge);
        assert!(
            fired[0].1 <= 10.0,
            "charge at {} should be near t=0",
            fired[0].1
        );
        assert_eq!(fired[1].0, RoastEventKind::Drop);
        assert!(
            (fired[1].1 - last_t).abs() <= 10.0,
            "drop at {} vs true {last_t}",
            fired[1].1
        );
    }

    #[test]
    fn disabled_charge_still_gates_drop_internally() {
        // autoMark.charge = 'off', autoMark.drop = 'on': the charge mark never
        // fires, but the internal charge signature still arms DROP correctly.
        let fixture = fixture_for_source_id("replay:fast-decaf").unwrap();
        let session = synth_session(fixture);
        let true_drop = PREHEAT_SEC as f64 + fixture.curve.last().unwrap().t;

        let mut rng = DetRng::new(0xBEEF_CAFE);
        let mut detector = AutoMarkDetector::new(false, true);
        let fired = feed_all(&mut detector, &session, Some(&mut rng));
        assert_eq!(fired.len(), 1, "only drop should fire, got {fired:?}");
        assert_eq!(fired[0].0, RoastEventKind::Drop);
        assert!((fired[0].1 - true_drop).abs() <= 10.0);
    }

    #[test]
    fn both_disabled_never_fires() {
        let fixture = fixture_for_source_id("replay:ethiopia-guji").unwrap();
        let session = synth_session(fixture);
        let mut detector = AutoMarkDetector::new(false, false);
        let fired = feed_all(&mut detector, &session, None);
        assert!(
            fired.is_empty(),
            "disabled detector must never fire, got {fired:?}"
        );
    }

    #[test]
    fn hot_stable_soak_with_noise_never_false_fires() {
        // 20 minutes sitting at a hot, noisy preheat: no charge, no drop.
        let mut rng = DetRng::new(0x5EED_0001);
        let session: Vec<(f64, f64)> = (0..1200).map(|t| (t as f64, 390.0)).collect();
        let mut detector = AutoMarkDetector::new(true, true);
        let fired = feed_all(&mut detector, &session, Some(&mut rng));
        assert!(fired.is_empty(), "stable soak must not fire, got {fired:?}");
    }

    #[test]
    fn cool_decline_is_not_a_charge() {
        // A decline that starts from a cool probe (< 250°F) is warm-up
        // fiddling, not a charge.
        let mut session = Vec::new();
        for t in 0..60 {
            session.push((t as f64, 200.0));
        }
        for t in 60..120 {
            session.push((t as f64, 200.0 - 2.5 * (t as f64 - 60.0)));
        }
        let mut detector = AutoMarkDetector::new(true, true);
        let fired = feed_all(&mut detector, &session, None);
        assert!(
            fired.is_empty(),
            "cool decline must not charge, got {fired:?}"
        );
    }

    #[test]
    fn each_kind_fires_at_most_once_per_lifetime() {
        // Two back-to-back roast shapes through ONE detector: the second
        // charge/drop signatures must not re-fire.
        let fixture = fixture_for_source_id("replay:fast-decaf").unwrap();
        let mut session = synth_session(fixture);
        let offset = session.last().unwrap().0 + 1.0;
        let again: Vec<(f64, f64)> = synth_session(fixture)
            .into_iter()
            .map(|(t, bt)| (t + offset, bt))
            .collect();
        session.extend(again);

        let mut detector = AutoMarkDetector::new(true, true);
        let fired = feed_all(&mut detector, &session, None);
        assert_eq!(
            fired.len(),
            2,
            "one charge + one drop across both shapes, got {fired:?}"
        );
        assert!(
            fired[0].1 < offset && fired[1].1 < offset,
            "both fires from the first roast"
        );
    }

    #[test]
    fn resumed_detector_never_reads_the_drop_plunge_as_a_second_charge() {
        // Crash recovery mid-roast: charge already persisted at (90, 390).
        // The resumed feed picks up in development — a rise to a hot peak,
        // then the bean-dump plunge. A FRESH detector fires CHARGE on that
        // plunge (the regression); a seeded one must fire DROP at most.
        let mut session: Vec<(f64, f64)> = Vec::new();
        for t in 0..120 {
            // development: 340 → 412°F over 2 minutes (+0.6°F/s)
            session.push((300.0 + t as f64, 340.0 + 0.6 * t as f64));
        }
        for t in 0..40 {
            // drop plunge at −3°F/s from the 412°F peak
            session.push((420.0 + t as f64, 412.0 - 3.0 * t as f64));
        }

        let mut detector = AutoMarkDetector::resumed(true, true, Some((90.0, 390.0)), false);
        let fired = feed_all(&mut detector, &session, None);
        assert_eq!(
            fired.len(),
            1,
            "only DROP may fire after resume, got {fired:?}"
        );
        assert_eq!(fired[0].0, RoastEventKind::Drop);
        assert!(
            (fired[0].1 - 420.0).abs() <= 10.0,
            "drop at {} should be near the 420s plunge start",
            fired[0].1
        );
    }

    #[test]
    fn resumed_detector_with_drop_marked_is_fully_disarmed() {
        // Crash after DROP (cool-down): the plunge continues on resume but
        // both kinds are already persisted — nothing may ever fire again.
        let mut session: Vec<(f64, f64)> = Vec::new();
        for t in 0..90 {
            session.push((500.0 + t as f64, (410.0 - 3.0 * t as f64).max(150.0)));
        }
        let mut detector = AutoMarkDetector::resumed(true, true, Some((90.0, 390.0)), true);
        let fired = feed_all(&mut detector, &session, None);
        assert!(
            fired.is_empty(),
            "disarmed resumed detector fired: {fired:?}"
        );
    }

    #[test]
    fn resumed_detector_without_markers_still_detects_a_charge() {
        // Crash during preheat (no markers yet): the seeded detector is
        // equivalent to a fresh one and the full charge+drop flow still works.
        let fixture = fixture_for_source_id("replay:fast-decaf").unwrap();
        let session = synth_session(fixture);
        let mut detector = AutoMarkDetector::resumed(true, true, None, false);
        let fired = feed_all(&mut detector, &session, None);
        assert_eq!(fired.len(), 2, "expected charge + drop, got {fired:?}");
        assert_eq!(fired[0].0, RoastEventKind::Charge);
        assert_eq!(fired[1].0, RoastEventKind::Drop);
    }

    #[test]
    fn post_charge_plunge_never_reads_as_drop() {
        // Right after charge BT is hot (≥380°F) AND falling steeply — the
        // recovery guard must keep DROP from firing during the plunge.
        // Truncate the session right after the turning point so a legitimate
        // drop never happens; assert no drop fires at all.
        let fixture = fixture_for_source_id("replay:ethiopia-guji").unwrap();
        let tp = fixture.markers.turning_point_sec.unwrap();
        let session: Vec<(f64, f64)> = synth_session(fixture)
            .into_iter()
            .take_while(|&(t, _)| t <= PREHEAT_SEC as f64 + tp + 20.0)
            .collect();
        let mut detector = AutoMarkDetector::new(true, true);
        let fired = feed_all(&mut detector, &session, None);
        assert_eq!(fired.len(), 1, "only the charge may fire, got {fired:?}");
        assert_eq!(fired[0].0, RoastEventKind::Charge);
    }
}
