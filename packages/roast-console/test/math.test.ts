import { describe, it, expect } from 'vitest';
import {
  deriveRoRSeries,
  trailingRoR,
  detectTurningPoint,
  computeDtr,
  weightLossPct,
  phaseBreakdown,
  projectDrop,
  rebaseSamples,
  fToC,
  formatTemp,
  mmss,
} from '../src/math';
import { BUNDLED_FIXTURES } from '../src/simulator';
import type { CurvePoint, LiveSample } from '../src/types';

// ---------------------------------------------------------------------------
// A verbatim copy of apps/droptime-app/convex/lib.ts `deriveRoR` — the parity
// oracle. deriveRoRSeries must match this element-for-element on every input.
// ---------------------------------------------------------------------------
type RefPt = { t: number; bt: number; ror?: number };
function refDeriveRoR(curve: RefPt[], windowSec = 30): RefPt[] {
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
    let lo = i;
    while (lo > 0 && (pt.t - curve[lo]!.t < windowSec || dropout[lo])) lo--;
    let hi = i;
    while (hi < curve.length - 1 && (curve[hi]!.t - pt.t < windowSec || dropout[hi])) hi++;
    if (dropout[lo] || dropout[hi]) return { ...pt, ror: undefined };
    const dt = (curve[hi]!.t - curve[lo]!.t) / 60;
    const ror = dt > 0 ? (curve[hi]!.bt - curve[lo]!.bt) / dt : 0;
    if (dt > 0.4 && ror === 0 && curve[hi]!.bt === curve[lo]!.bt && pt.bt === curve[lo]!.bt) {
      return { ...pt, ror: undefined };
    }
    return { ...pt, ror: Math.round(ror * 10) / 10 };
  });
}

const rors = (pts: Array<{ ror?: number }>): Array<number | undefined> => pts.map((p) => p.ror);

describe('deriveRoRSeries — parity with convex/lib.ts', () => {
  const cases: Record<string, CurvePoint[]> = {
    'clean ramp': Array.from({ length: 25 }, (_, i) => ({ t: i * 5, bt: 300 + 0.8 * i * 5 })),
    'flat-line': Array.from({ length: 13 }, (_, i) => ({ t: i * 5, bt: 350 })),
    'dropout bt<=0': [
      { t: 0, bt: 300 },
      { t: 5, bt: 305 },
      { t: 10, bt: 0 },
      { t: 15, bt: 315 },
      { t: 20, bt: 320 },
      { t: 25, bt: 326 },
      { t: 30, bt: 332 },
    ],
    'impossible jump': [
      { t: 0, bt: 300 },
      { t: 5, bt: 306 },
      { t: 10, bt: 312 },
      { t: 15, bt: 500 }, // +188°F/5s ≈ 2256°F/min → dropout
      { t: 20, bt: 324 },
      { t: 25, bt: 330 },
      { t: 30, bt: 336 },
    ],
  };

  for (const [name, curve] of Object.entries(cases)) {
    it(`matches the oracle: ${name}`, () => {
      expect(rors(deriveRoRSeries(curve))).toEqual(rors(refDeriveRoR(curve)));
    });
  }

  it('matches the oracle on every bundled fixture', () => {
    for (const fx of Object.values(BUNDLED_FIXTURES)) {
      const curve: CurvePoint[] = fx.curve.map((p) => ({ t: p.t, bt: p.bt }));
      expect(rors(deriveRoRSeries(curve))).toEqual(rors(refDeriveRoR(curve)));
    }
  });
});

describe('deriveRoRSeries — rules', () => {
  it('a steady 0.8°F/s ramp reads ~48°F/min in the interior', () => {
    const curve: CurvePoint[] = Array.from({ length: 25 }, (_, i) => ({ t: i * 5, bt: 300 + 0.8 * (i * 5) }));
    const out = deriveRoRSeries(curve);
    const mid = out.find((p) => p.t === 60)!;
    expect(mid.ror).toBe(48);
  });

  it('a flat-lined probe yields undefined, never a 0 stall', () => {
    const curve: CurvePoint[] = Array.from({ length: 13 }, (_, i) => ({ t: i * 5, bt: 350 }));
    expect(deriveRoRSeries(curve).every((p) => p.ror === undefined)).toBe(true);
  });

  it('excludes a sub-zero probe reading', () => {
    const curve: CurvePoint[] = [
      { t: 0, bt: 300 },
      { t: 5, bt: 305 },
      { t: 10, bt: 0 },
      { t: 15, bt: 315 },
    ];
    expect(deriveRoRSeries(curve).find((p) => p.t === 10)!.ror).toBeUndefined();
  });

  it('excludes a physically impossible jump', () => {
    const curve: CurvePoint[] = [
      { t: 0, bt: 300 },
      { t: 5, bt: 306 },
      { t: 10, bt: 500 },
      { t: 15, bt: 312 },
    ];
    expect(deriveRoRSeries(curve).find((p) => p.t === 10)!.ror).toBeUndefined();
  });

  it('colombia-stall shows a visible RoR crash after dry-end', () => {
    const fx = BUNDLED_FIXTURES['colombia-stall']!;
    const curve: CurvePoint[] = fx.curve.map((p) => ({ t: p.t, bt: p.bt }));
    const series = deriveRoRSeries(curve);
    const dryEnd = fx.markers.dryEndSec!;
    const inStall = series.filter((p) => p.t >= dryEnd + 20 && p.t <= dryEnd + 60 && p.ror !== undefined);
    const beforeDry = series.filter((p) => p.t >= dryEnd - 60 && p.t <= dryEnd && p.ror !== undefined);
    const minStall = Math.min(...inStall.map((p) => p.ror!));
    const avgBefore = beforeDry.reduce((a, p) => a + p.ror!, 0) / beforeDry.length;
    expect(minStall).toBeLessThan(3); // crashed toward a stall
    expect(avgBefore).toBeGreaterThan(10); // momentum was healthy beforehand
  });
});

describe('trailingRoR', () => {
  it('is stable under 1–2°F sampling noise (least-squares, not two-point)', () => {
    // True slope 0.8°F/s = 48°F/min, plus alternating ±1.5°F probe noise.
    const pts: CurvePoint[] = Array.from({ length: 31 }, (_, i) => ({
      t: i,
      bt: 300 + 0.8 * i + (i % 2 === 0 ? 1.5 : -1.5),
    }));
    const r = trailingRoR(pts)!;
    expect(Math.abs(r - 48)).toBeLessThan(8);
  });

  it('is undefined with fewer than two samples', () => {
    expect(trailingRoR([{ t: 0, bt: 300 }])).toBeUndefined();
  });
});

describe('detectTurningPoint', () => {
  it('finds the coolest point once BT has risen past it', () => {
    const down: CurvePoint[] = Array.from({ length: 7 }, (_, i) => ({ t: i * 5, bt: 400 - 3 * (i * 5) }));
    const up: CurvePoint[] = Array.from({ length: 6 }, (_, i) => ({ t: 35 + i * 5, bt: 310 + 2 * (i * 5 + 5) }));
    const tp = detectTurningPoint([...down, ...up]);
    expect(tp).toEqual({ t: 30, bt: 310 });
  });

  it('returns undefined while BT is still falling', () => {
    const down: CurvePoint[] = Array.from({ length: 8 }, (_, i) => ({ t: i * 5, bt: 400 - 3 * (i * 5) }));
    expect(detectTurningPoint(down)).toBeUndefined();
  });
});

describe('computeDtr / weightLossPct — parity', () => {
  it('computeDtr(585, 720) === 0.188', () => {
    expect(computeDtr(585, 720)).toBe(0.188);
  });
  it('computeDtr is undefined without both markers or with drop<=0', () => {
    expect(computeDtr(undefined, 720)).toBeUndefined();
    expect(computeDtr(585, undefined)).toBeUndefined();
    expect(computeDtr(585, 0)).toBeUndefined();
  });
  it('weightLossPct(30, 25.5) === 15', () => {
    expect(weightLossPct(30, 25.5)).toBe(15);
  });
  it('weightLossPct guards missing/zero charge', () => {
    expect(weightLossPct(0, 25)).toBeUndefined();
    expect(weightLossPct(30, undefined)).toBeUndefined();
    expect(weightLossPct(undefined, 25)).toBeUndefined();
  });
});

describe('phaseBreakdown', () => {
  it('is pure preheat before charge', () => {
    expect(phaseBreakdown({}, 0, false)).toEqual({ phase: 'preheat' });
  });

  it('splits a completed roast into drying/maillard/development', () => {
    const pb = phaseBreakdown({ dryEndSec: 300, fcStartSec: 560, dropSec: 660 }, 660, true);
    expect(pb.phase).toBe('cooling');
    expect(pb.dryingPct).toBe(45.5);
    expect(pb.maillardPct).toBe(39.4);
    expect(pb.developmentPct).toBe(15.2);
    expect(pb.dtr).toBe(0.152);
  });

  it('reports live proportions of elapsed time mid-roast', () => {
    const pb = phaseBreakdown({ dryEndSec: 300 }, 400, true);
    expect(pb.phase).toBe('maillard');
    expect(pb.dryingPct).toBe(75);
    expect(pb.maillardPct).toBe(25);
    expect(pb.developmentPct).toBe(0);
    expect(pb.dtr).toBeUndefined();
  });
});

describe('projectDrop', () => {
  it('projects a plausible drop time from mid-roast on a fixture', () => {
    const fx = BUNDLED_FIXTURES['ethiopia-guji']!;
    const upToFc: CurvePoint[] = fx.curve
      .filter((p) => p.t <= fx.markers.fcStartSec!)
      .map((p) => ({ t: p.t, bt: p.bt }));
    const withRor = deriveRoRSeries(upToFc);
    const proj = projectDrop(withRor, fx.markers.dropTempF!);
    expect(proj).toBeDefined();
    expect(proj!.projectedDropSec).toBeGreaterThan(fx.markers.fcStartSec!);
    // The true drop is 660s; a mid-roast extrapolation should land in the ballpark.
    expect(proj!.projectedDropSec).toBeGreaterThan(600);
    expect(proj!.projectedDropSec).toBeLessThan(820);
    expect(['low', 'medium', 'high']).toContain(proj!.confidence);
  });

  it('returns undefined pre-charge', () => {
    const pre: CurvePoint[] = [
      { t: -40, bt: 388 },
      { t: -20, bt: 386 },
    ];
    expect(projectDrop(pre, 415)).toBeUndefined();
  });

  it('reports high confidence at the current time when target already reached', () => {
    const pts: CurvePoint[] = [
      { t: 600, bt: 410, ror: 8 },
      { t: 660, bt: 418, ror: 6 },
    ];
    const proj = projectDrop(pts, 415);
    expect(proj).toEqual({ projectedDropSec: 660, targetDropTempF: 415, confidence: 'high' });
  });
});

describe('rebaseSamples', () => {
  const samples: LiveSample[] = [];
  for (let sec = 0; sec <= 120; sec += 5) {
    // flat 390°F preheat, then a rising 0.5°F/s ramp after charge at sec=60
    const bt = sec < 60 ? 390 : 390 + 0.5 * (sec - 60);
    samples.push({ seq: sec / 5, sessionSec: sec, btF: bt });
  }

  it('rebases to seconds-from-charge with negative preheat t', () => {
    const curve = rebaseSamples(samples, 60);
    expect(curve[0]!.t).toBe(-60);
    expect(curve.find((p) => p.t === 0)).toBeDefined();
    expect(curve[curve.length - 1]!.t).toBe(60);
    expect(curve.some((p) => p.t < 0)).toBe(true);
  });

  it('has a defined trailing RoR at the leading edge', () => {
    const curve = rebaseSamples(samples, 60);
    const last = curve[curve.length - 1]!;
    expect(last.ror).toBeDefined();
    // trailing slope of the 0.5°F/s ramp ≈ 30°F/min
    expect(Math.abs(last.ror! - 30)).toBeLessThan(6);
  });

  it('offset defaults to 0 when charge is not yet marked', () => {
    const curve = rebaseSamples(samples, undefined);
    expect(curve[0]!.t).toBe(0);
    expect(curve[curve.length - 1]!.t).toBe(120);
  });
});

describe('units & formatting', () => {
  it('fToC', () => {
    expect(fToC(32)).toBeCloseTo(0, 6);
    expect(fToC(212)).toBeCloseTo(100, 6);
  });
  it('formatTemp', () => {
    expect(formatTemp(412.4, 'F')).toBe('412°F');
    expect(formatTemp(212, 'C')).toBe('100.0°C');
    expect(formatTemp(412.44, 'F', 1)).toBe('412.4°F');
  });
  it('mmss parity', () => {
    expect(mmss(0)).toBe('0:00');
    expect(mmss(65)).toBe('1:05');
    expect(mmss(600)).toBe('10:00');
    expect(mmss(-5)).toBe('0:00');
  });
});
