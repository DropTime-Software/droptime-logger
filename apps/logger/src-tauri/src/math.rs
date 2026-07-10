//! Numeric helpers with STRICT semantic parity to
//! `packages/roast-console/src/math/index.ts` (which itself keeps parity with
//! the platform server math). If a formula changes there, change it here.

/// JS `Math.round` parity: half-up toward +∞ (`Math.round(-1.5) === -1`),
/// unlike Rust's `f64::round` (half away from zero).
fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// Development-time ratio = (drop − fcStart) / drop, to 3dp.
/// Parity with `computeDtr` in roast-console/math.
pub fn compute_dtr(fc_start_sec: Option<f64>, drop_sec: Option<f64>) -> Option<f64> {
    let fc = fc_start_sec?;
    let drop = drop_sec?;
    if drop <= 0.0 {
        return None;
    }
    Some(js_round(((drop - fc) / drop) * 1000.0) / 1000.0)
}

/// Roast weight loss as a one-decimal percentage.
/// Parity with `weightLossPct` in roast-console/math.
pub fn weight_loss_pct(charge_lb: Option<f64>, drop_lb: Option<f64>) -> Option<f64> {
    let charge = charge_lb?;
    let drop = drop_lb?;
    if charge <= 0.0 {
        return None;
    }
    Some(js_round(((charge - drop) / charge) * 1000.0) / 10.0)
}

/// Detect the turning point over rebased points `(t = seconds-from-charge, bt)`:
/// the coolest BT after charge, kept only once BT has demonstrably risen again
/// (some later point > min + 1°F). Parity with `detectTurningPoint`:
/// skips `t < 0` (preheat) and `bt <= 0` (dropout); strict `<` keeps the FIRST
/// minimum; returns None while BT is still falling.
pub fn detect_turning_point(points: &[(f64, f64)]) -> Option<(f64, f64)> {
    let mut min: Option<(f64, f64)> = None;
    for &(t, bt) in points {
        if t < 0.0 || bt <= 0.0 {
            continue;
        }
        match min {
            Some((_, mbt)) if bt >= mbt => {}
            _ => min = Some((t, bt)),
        }
    }
    let (at, abt) = min?;
    let rose = points.iter().any(|&(t, bt)| t > at && bt > abt + 1.0);
    if rose {
        Some((at, abt))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dtr_parity() {
        // (660 - 560) / 660 = 0.1515... → 0.152
        assert_eq!(compute_dtr(Some(560.0), Some(660.0)), Some(0.152));
        assert_eq!(compute_dtr(None, Some(660.0)), None);
        assert_eq!(compute_dtr(Some(560.0), None), None);
        assert_eq!(compute_dtr(Some(560.0), Some(0.0)), None);
    }

    #[test]
    fn weight_loss_parity() {
        // (24 - 20.4) / 24 = 15.0%
        assert_eq!(weight_loss_pct(Some(24.0), Some(20.4)), Some(15.0));
        // (30 - 25.9) / 30 = 13.666...% → 13.7
        assert_eq!(weight_loss_pct(Some(30.0), Some(25.9)), Some(13.7));
        assert_eq!(weight_loss_pct(Some(0.0), Some(1.0)), None);
        assert_eq!(weight_loss_pct(None, Some(1.0)), None);
    }

    #[test]
    fn turning_point_basic() {
        let pts: Vec<(f64, f64)> = vec![
            (-5.0, 100.0), // preheat: ignored even though it is the global min
            (0.0, 388.0),
            (30.0, 250.0),
            (60.0, 180.0),
            (82.0, 173.0),
            (90.0, 175.5),
            (120.0, 190.0),
        ];
        assert_eq!(detect_turning_point(&pts), Some((82.0, 173.0)));
    }

    #[test]
    fn turning_point_none_while_falling() {
        let pts: Vec<(f64, f64)> = vec![(0.0, 388.0), (30.0, 250.0), (60.0, 180.0)];
        assert_eq!(detect_turning_point(&pts), None);
        // +1°F guard: a rise of exactly 1.0 is not "rose"
        let flat: Vec<(f64, f64)> = vec![(0.0, 200.0), (30.0, 180.0), (60.0, 181.0)];
        assert_eq!(detect_turning_point(&flat), None);
    }

    #[test]
    fn turning_point_skips_dropouts_and_keeps_first_min() {
        let pts: Vec<(f64, f64)> = vec![
            (0.0, 388.0),
            (30.0, 0.0), // dropout
            (60.0, 173.0),
            (90.0, 173.0), // equal min later → first wins
            (120.0, 200.0),
        ];
        assert_eq!(detect_turning_point(&pts), Some((60.0, 173.0)));
    }
}
