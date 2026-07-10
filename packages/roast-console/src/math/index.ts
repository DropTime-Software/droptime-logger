/**
 * Curve math for the roast console — the numeric core shared by the live logger,
 * the demo web app and the companion viewer. Pure, dependency-free, unit-tested.
 *
 * Canonical units: temperature °F, time seconds, RoR °F/min. The RoR derivation,
 * DTR and weight-loss helpers keep BYTE-FOR-BYTE numeric parity with the platform
 * server math in apps/droptime-app/convex/lib.ts (a platform contract) so a curve
 * derived live in the logger matches the one the backend re-derives after sync.
 */
import type {
  CurvePoint,
  DropProjection,
  LiveSample,
  PhaseBreakdown,
  RoastMarkersSec,
  RoastPhase,
} from '../types';

// ---------------------------------------------------------------------------
// Rate-of-rise
// ---------------------------------------------------------------------------

/**
 * Derive a rate-of-rise (°F/min) series from bean temperature using a SYMMETRIC
 * window (default ±windowSec) to smooth probe noise. Parity with convex/lib.ts
 * `deriveRoR`:
 *   - a bean probe reading <= 0°F is a dropout (excluded);
 *   - an inter-sample swing > 120°F/min is physically impossible → dropout;
 *   - a probe flat-lined across the whole window reads a hard 0 with zero
 *     variance → ror `undefined` (not 0, which would masquerade as a real stall);
 *   - values rounded to 0.1°F/min.
 * A point that already carries `ror` is passed through untouched.
 */
export function deriveRoRSeries(curve: CurvePoint[], windowSec = 30): CurvePoint[] {
  const n = curve.length;
  const dropout = curve.map((pt, i) => {
    if (pt.bt <= 0) return true;
    if (i === 0) return false;
    const prev = curve[i - 1]!;
    const dt = Math.max(1, pt.t - prev.t);
    const rate = Math.abs(pt.bt - prev.bt) / (dt / 60);
    return rate > 120;
  });
  return curve.map((pt, i) => {
    if (pt.ror !== undefined) return pt;
    if (dropout[i]) return { ...pt, ror: undefined };
    // Widen to the nearest non-dropout neighbours ~windowSec on each side.
    let lo = i;
    while (lo > 0 && (pt.t - curve[lo]!.t < windowSec || dropout[lo])) lo--;
    let hi = i;
    while (hi < n - 1 && (curve[hi]!.t - pt.t < windowSec || dropout[hi])) hi++;
    if (dropout[lo] || dropout[hi]) return { ...pt, ror: undefined };
    const loPt = curve[lo]!;
    const hiPt = curve[hi]!;
    const dt = (hiPt.t - loPt.t) / 60;
    const ror = dt > 0 ? (hiPt.bt - loPt.bt) / dt : 0;
    if (dt > 0.4 && ror === 0 && hiPt.bt === loPt.bt && pt.bt === loPt.bt) {
      return { ...pt, ror: undefined };
    }
    return { ...pt, ror: Math.round(ror * 10) / 10 };
  });
}

/** Least-squares BT slope (°F/min) over a set of points; undefined if degenerate. */
function lsSlopePerMin(pts: CurvePoint[]): number | undefined {
  const n = pts.length;
  if (n < 2) return undefined;
  let sx = 0;
  let sy = 0;
  let sxx = 0;
  let sxy = 0;
  for (const p of pts) {
    sx += p.t;
    sy += p.bt;
    sxx += p.t * p.t;
    sxy += p.t * p.bt;
  }
  const denom = n * sxx - sx * sx;
  if (denom === 0) return undefined; // all samples at the same instant
  const slopePerSec = (n * sxy - sx * sy) / denom;
  return slopePerSec * 60;
}

/**
 * Leading-edge live RoR (°F/min). Uses a least-squares slope over the trailing
 * `windowSec`, NOT a two-point delta, so it stays stable under 1–2s sampling
 * noise. Dropout samples (bt <= 0) are excluded. Undefined until there are two
 * usable samples spanning nonzero time.
 */
export function trailingRoR(points: CurvePoint[], windowSec = 30): number | undefined {
  if (points.length < 2) return undefined;
  const tEnd = points[points.length - 1]!.t;
  const win = points.filter((p) => p.bt > 0 && p.t <= tEnd && p.t >= tEnd - windowSec);
  const slope = lsSlopePerMin(win);
  return slope === undefined ? undefined : Math.round(slope * 10) / 10;
}

// ---------------------------------------------------------------------------
// Turning point
// ---------------------------------------------------------------------------

/**
 * Detect the turning point — the coolest bean temperature after charge, where BT
 * stops falling and begins to climb. Returns undefined while BT is still falling
 * (the minimum is the most recent sample) so live callers don't latch a TP early.
 */
export function detectTurningPoint(points: CurvePoint[]): { t: number; bt: number } | undefined {
  let min: { t: number; bt: number } | undefined;
  for (const p of points) {
    if (p.t < 0) continue; // pre-charge preheat
    if (p.bt <= 0) continue; // dropout
    if (min === undefined || p.bt < min.bt) min = { t: p.t, bt: p.bt };
  }
  if (min === undefined) return undefined;
  const anchor = min;
  const rose = points.some((p) => p.t > anchor.t && p.bt > anchor.bt + 1);
  return rose ? anchor : undefined;
}

// ---------------------------------------------------------------------------
// DTR & weight loss (parity with convex/lib.ts)
// ---------------------------------------------------------------------------

/** Development-time ratio = (drop − fcStart) / drop, to 3dp. Parity with lib.ts. */
export function computeDtr(fcStartSec?: number, dropSec?: number): number | undefined {
  if (fcStartSec === undefined || dropSec === undefined || dropSec <= 0) return undefined;
  return Math.round(((dropSec - fcStartSec) / dropSec) * 1000) / 1000;
}

/** Roast weight loss as a one-decimal percentage. Parity with lib.ts. */
export function weightLossPct(chargeLb?: number, dropLb?: number): number | undefined {
  if (chargeLb === undefined || chargeLb <= 0 || dropLb === undefined) return undefined;
  return Math.round(((chargeLb - dropLb) / chargeLb) * 1000) / 10;
}

// ---------------------------------------------------------------------------
// Phase breakdown
// ---------------------------------------------------------------------------

const clamp = (v: number, lo: number, hi: number): number => Math.max(lo, Math.min(hi, v));
const round1 = (n: number): number => Math.round(n * 10) / 10;

/**
 * Live phase breakdown. Before charge everything is preheat. Once charged the
 * current phase advances as each marker is crossed (drying → maillard →
 * development → cooling), and drying/maillard/development are reported as
 * percentages of time-since-charge (of the drop time once dropped, else of the
 * elapsed time). Live DTR appears once fc_start exists.
 */
export function phaseBreakdown(
  markers: RoastMarkersSec,
  elapsedT: number,
  charged: boolean,
): PhaseBreakdown {
  if (!charged) return { phase: 'preheat' };

  const { dryEndSec, fcStartSec, dropSec } = markers;

  let phase: RoastPhase;
  if (dropSec !== undefined && elapsedT >= dropSec) {
    phase = 'cooling';
  } else if (fcStartSec !== undefined && elapsedT >= fcStartSec) {
    phase = 'development';
  } else if (dryEndSec !== undefined && elapsedT >= dryEndSec) {
    phase = 'maillard';
  } else {
    phase = 'drying';
  }

  const result: PhaseBreakdown = { phase };

  const total = dropSec ?? elapsedT;
  if (total > 0) {
    const b1 = clamp(dryEndSec ?? total, 0, total); // end of drying
    const b2 = clamp(fcStartSec ?? total, b1, total); // end of maillard
    result.dryingPct = round1((b1 / total) * 100);
    result.maillardPct = round1(((b2 - b1) / total) * 100);
    result.developmentPct = round1(((total - b2) / total) * 100);
  }

  if (fcStartSec !== undefined) {
    const dtr = computeDtr(fcStartSec, dropSec ?? elapsedT);
    if (dtr !== undefined) result.dtr = dtr;
  }

  return result;
}

// ---------------------------------------------------------------------------
// Drop projection
// ---------------------------------------------------------------------------

interface LinFit {
  slope: number;
  intercept: number;
  r2: number;
}

function linreg(xs: number[], ys: number[]): LinFit | undefined {
  const n = xs.length;
  if (n < 2) return undefined;
  let sx = 0;
  let sy = 0;
  let sxx = 0;
  let sxy = 0;
  for (let i = 0; i < n; i++) {
    const x = xs[i]!;
    const y = ys[i]!;
    sx += x;
    sy += y;
    sxx += x * x;
    sxy += x * y;
  }
  const denom = n * sxx - sx * sx;
  if (denom === 0) return undefined;
  const slope = (n * sxy - sx * sy) / denom;
  const intercept = (sy - slope * sx) / n;
  const meanY = sy / n;
  let ssRes = 0;
  let ssTot = 0;
  for (let i = 0; i < n; i++) {
    const x = xs[i]!;
    const y = ys[i]!;
    const pred = slope * x + intercept;
    ssRes += (y - pred) ** 2;
    ssTot += (y - meanY) ** 2;
  }
  const r2 = ssTot === 0 ? 1 : Math.max(0, 1 - ssRes / ssTot);
  return { slope, intercept, r2 };
}

const PROJECT_WINDOW_SEC = 90;

/**
 * Project the seconds-from-charge at which BT reaches `targetDropTempF` by
 * fitting the recent (~90s) RoR decay and integrating BT forward. RoR is
 * modelled as a line ror(τ) = rorNow + m·(τ − now); BT rises by ∫ror/60, giving
 * a quadratic solved for the crossing time. Confidence blends fit quality (R²)
 * with the extrapolation horizon. Returns undefined pre-charge, at a dropout
 * leading edge, or when there is no positive RoR trend to extrapolate.
 */
export function projectDrop(
  points: CurvePoint[],
  targetDropTempF: number,
): DropProjection | undefined {
  if (points.length === 0) return undefined;
  const last = points[points.length - 1]!;
  if (last.t < 0) return undefined; // pre-charge
  const tNow = last.t;
  const btNow = last.bt;
  if (btNow <= 0) return undefined; // dropout at the leading edge

  if (targetDropTempF <= btNow) {
    return { projectedDropSec: Math.round(tNow), targetDropTempF, confidence: 'high' };
  }

  const win = points.filter((p) => p.t >= tNow - PROJECT_WINDOW_SEC && p.ror !== undefined && p.bt > 0);

  let rorNow: number | undefined;
  let m = 0; // RoR slope, °F/min per second
  let r2 = 0;
  let usedFallback = false;

  if (win.length >= 3) {
    const fit = linreg(
      win.map((p) => p.t),
      win.map((p) => p.ror as number),
    );
    if (fit) {
      m = fit.slope;
      rorNow = fit.intercept + fit.slope * tNow;
      r2 = fit.r2;
    }
  }
  if (rorNow === undefined) {
    rorNow = trailingRoR(points);
    m = 0;
    r2 = 0;
    usedFallback = true;
  }
  if (rorNow === undefined || rorNow <= 0) return undefined;

  const deltaT = targetDropTempF - btNow; // > 0
  // rorNow·x + (m/2)·x² = 60·deltaT ; solve for the smallest positive x.
  let x: number | undefined;
  if (Math.abs(m) < 1e-6) {
    x = (60 * deltaT) / rorNow;
  } else {
    const disc = rorNow * rorNow + 120 * m * deltaT;
    if (disc >= 0) {
      const sq = Math.sqrt(disc);
      const roots = [(-rorNow + sq) / m, (-rorNow - sq) / m].filter((v) => v > 0 && Number.isFinite(v));
      if (roots.length) x = Math.min(...roots);
    }
    if (x === undefined) {
      // Under the current decay RoR fades before reaching target — fall back to
      // an optimistic constant-RoR estimate so the UI still shows a horizon.
      x = (60 * deltaT) / rorNow;
      usedFallback = true;
    }
  }
  if (x === undefined || !Number.isFinite(x) || x <= 0) return undefined;

  const horizon = x;
  let confidence: DropProjection['confidence'];
  if (usedFallback || win.length < 4) confidence = 'low';
  else if (r2 >= 0.6 && horizon <= 150) confidence = 'high';
  else if (r2 >= 0.3 && horizon <= 300) confidence = 'medium';
  else confidence = 'low';

  return { projectedDropSec: Math.round(tNow + x), targetDropTempF, confidence };
}

// ---------------------------------------------------------------------------
// Rebase raw samples → chart curve
// ---------------------------------------------------------------------------

/**
 * Rebase raw live samples to chart points (t = sessionSec − chargeSessionSec;
 * pre-charge samples get negative t). RoR is symmetric where a full forward
 * window exists and trailing (least-squares) at the leading edge where it does
 * not — matching how the live chart and the post-roast re-derivation line up.
 */
export function rebaseSamples(
  samples: LiveSample[],
  chargeSessionSec: number | undefined,
  rorWindowSec = 30,
): CurvePoint[] {
  if (samples.length === 0) return [];
  const offset = chargeSessionSec ?? 0;
  const sorted = [...samples].sort((a, b) => a.sessionSec - b.sessionSec);
  const base: CurvePoint[] = sorted.map((s) => {
    const pt: CurvePoint = { t: s.sessionSec - offset, bt: s.btF };
    if (s.etF !== undefined) pt.et = s.etF;
    if (s.heater !== undefined) pt.heater = s.heater;
    if (s.fan !== undefined) pt.fan = s.fan;
    if (s.drum !== undefined) pt.drum = s.drum;
    return pt;
  });

  const series = deriveRoRSeries(base, rorWindowSec);
  const tEnd = base[base.length - 1]!.t;
  return series.map((pt, i) => {
    const bp = base[i]!;
    if (tEnd - bp.t < rorWindowSec) {
      // Leading edge: no full forward window → trailing least-squares slope.
      const win = base.filter((p) => p.bt > 0 && p.t <= bp.t && p.t >= bp.t - rorWindowSec);
      const slope = lsSlopePerMin(win);
      const ror = slope === undefined ? undefined : Math.round(slope * 10) / 10;
      const out: CurvePoint = { ...pt };
      if (ror === undefined) delete out.ror;
      else out.ror = ror;
      return out;
    }
    return pt;
  });
}

// ---------------------------------------------------------------------------
// Units & formatting
// ---------------------------------------------------------------------------

export function fToC(f: number): number {
  return ((f - 32) * 5) / 9;
}

/** Format a °F value in the requested unit, e.g. "412°F" / "211.1°C". */
export function formatTemp(f: number, unit: 'F' | 'C', digits?: number): string {
  const d = digits ?? (unit === 'C' ? 1 : 0);
  const v = unit === 'C' ? fToC(f) : f;
  return `${v.toFixed(d)}°${unit}`;
}

/** Seconds → "m:ss". Parity with convex/lib.ts `mmss`. */
export function mmss(totalSec: number): string {
  const s = Math.max(0, Math.round(totalSec));
  const m = Math.floor(s / 60);
  const r = s % 60;
  return `${m}:${`${r}`.padStart(2, '0')}`;
}
