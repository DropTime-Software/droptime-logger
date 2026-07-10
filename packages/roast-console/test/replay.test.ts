import { describe, it, expect } from 'vitest';
import {
  upcomingCues,
  formatCue,
  downsampleCurve,
  referenceEventLabel,
  REFERENCE_CUE_WINDOW_SEC,
  REFERENCE_CURVE_MAX_POINTS,
  type ReferenceCue,
} from '../src/replay';
import type { CurvePoint, RoastMarkersSec } from '../src/types';

// A representative ~11-min washed reference (seconds-from-charge markers).
const REF: RoastMarkersSec = {
  turningPointSec: 60,
  turningPointTempF: 200,
  dryEndSec: 300,
  fcStartSec: 540,
  fcEndSec: 600,
  dropSec: 660,
  dropTempF: 415,
  chargeTempF: 380,
};

// A synthetic curve rising 300 → 420 °F across 0..660s at 1 Hz.
const CURVE: CurvePoint[] = Array.from({ length: 661 }, (_, i) => ({
  t: i,
  bt: 300 + (120 * i) / 660,
}));

const kinds = (cues: ReferenceCue[]) => cues.map((c) => c.kind);

describe('upcomingCues — window + ordering', () => {
  it('returns only markers inside the look-ahead window, soonest first', () => {
    // nowT = 520: fc_start(540, +20), fc_end(600, +80) — only fc_start within 45s.
    const cues = upcomingCues(REF, CURVE, 520, 45);
    expect(kinds(cues)).toEqual(['fc_start']);
    expect(cues[0]!.secondsUntil).toBe(20);
    expect(cues[0]!.passed).toBe(false);
    expect(cues[0]!.terminal).toBe(false);
  });

  it('includes multiple markers when several fall in the window, chronological', () => {
    // nowT = 560, window 120: fc_end(600,+40), drop(660,+100).
    const cues = upcomingCues(REF, CURVE, 560, 120);
    expect(kinds(cues)).toEqual(['fc_end', 'drop']);
    expect(cues[0]!.secondsUntil).toBe(40);
    expect(cues[1]!.secondsUntil).toBe(100);
  });

  it('defaults the window to REFERENCE_CUE_WINDOW_SEC (45s)', () => {
    expect(REFERENCE_CUE_WINDOW_SEC).toBe(45);
    // dry_end at 300; at nowT 256 it is +44 (in), at 254 it would be +46 (out).
    expect(kinds(upcomingCues(REF, CURVE, 256))).toEqual(['dry_end']);
    expect(kinds(upcomingCues(REF, CURVE, 254))).toEqual([]);
  });
});

describe('upcomingCues — window edges', () => {
  it('includes a marker exactly at the far edge (secondsUntil === windowSec)', () => {
    // dry_end at 300, nowT 255 → +45 == window.
    const cues = upcomingCues(REF, CURVE, 255, 45);
    expect(kinds(cues)).toEqual(['dry_end']);
    expect(cues[0]!.secondsUntil).toBe(45);
  });

  it('excludes a marker one second beyond the far edge', () => {
    // dry_end at 300, nowT 254 → +46 > window.
    expect(upcomingCues(REF, CURVE, 254, 45)).toEqual([]);
  });

  it('includes a marker exactly at nowT (secondsUntil === 0, passed=true, not terminal)', () => {
    const cues = upcomingCues(REF, CURVE, 300, 45); // dry_end at 300 exactly
    expect(kinds(cues)).toEqual(['dry_end']);
    expect(cues[0]!.secondsUntil).toBe(0);
    expect(cues[0]!.passed).toBe(true);
    expect(cues[0]!.terminal).toBe(false);
  });

  it('excludes markers already in the past (except the terminal drop rule)', () => {
    // nowT 610: fc_start/fc_end are past; only drop(660,+50) is >45 out → empty.
    expect(upcomingCues(REF, CURVE, 610, 45)).toEqual([]);
  });
});

describe('upcomingCues — before charge', () => {
  it('returns [] when nowT is negative (preheat)', () => {
    expect(upcomingCues(REF, CURVE, -30, 45)).toEqual([]);
    expect(upcomingCues(REF, CURVE, -0.5, 45)).toEqual([]);
  });

  it('returns [] when nowT is not finite', () => {
    expect(upcomingCues(REF, CURVE, Number.NaN, 45)).toEqual([]);
    expect(upcomingCues(REF, CURVE, Number.POSITIVE_INFINITY, 45)).toEqual([]);
  });

  it('starts cueing at exactly charge (nowT === 0)', () => {
    // turning_point at 60 is the only marker within 45s of t=0? No — 60 > 45.
    expect(upcomingCues(REF, CURVE, 0, 45)).toEqual([]);
    // widen the window and it appears.
    expect(kinds(upcomingCues(REF, CURVE, 0, 60))).toEqual(['turning_point']);
  });
});

describe('upcomingCues — missing markers', () => {
  it('skips markers that are undefined', () => {
    const sparse: RoastMarkersSec = { fcStartSec: 540, dropSec: 660 };
    // nowT 500, window 120: fc_start(+40), drop(+160 out) → only fc_start.
    expect(kinds(upcomingCues(sparse, CURVE, 500, 120))).toEqual(['fc_start']);
    // nowT 520, window 200: fc_start(+20), drop(+140) both in.
    expect(kinds(upcomingCues(sparse, CURVE, 520, 200))).toEqual(['fc_start', 'drop']);
  });

  it('skips non-finite marker values', () => {
    const bad = { dryEndSec: Number.NaN, fcStartSec: 540, dropSec: 660 } as RoastMarkersSec;
    expect(kinds(upcomingCues(bad, CURVE, 500, 60))).toEqual(['fc_start']);
  });

  it('returns [] when the reference has no time-bearing markers', () => {
    expect(upcomingCues({ chargeTempF: 380 }, CURVE, 120, 45)).toEqual([]);
  });
});

describe('upcomingCues — reference shorter than the live roast (terminal)', () => {
  it('emits a single terminal drop cue once nowT passes the reference drop', () => {
    const cues = upcomingCues(REF, CURVE, 700, 45); // dropSec 660, nowT 700
    expect(cues.length).toBe(1);
    const cue = cues[0]!;
    expect(cue.kind).toBe('drop');
    expect(cue.terminal).toBe(true);
    expect(cue.passed).toBe(true);
    expect(cue.atT).toBe(660);
    expect(cue.secondsUntil).toBe(-40);
  });

  it('does NOT go terminal exactly at the drop (drop shows as an imminent cue)', () => {
    const cues = upcomingCues(REF, CURVE, 660, 45);
    expect(kinds(cues)).toEqual(['drop']);
    expect(cues[0]!.terminal).toBe(false);
    expect(cues[0]!.secondsUntil).toBe(0);
  });

  it('does not emit a terminal cue when the reference has no drop marker', () => {
    const noDrop: RoastMarkersSec = { fcStartSec: 540, fcEndSec: 600 };
    expect(upcomingCues(noDrop, CURVE, 900, 45)).toEqual([]);
  });
});

describe('upcomingCues — reference BT lookup', () => {
  it('interpolates reference BT at the marker time when covered by the curve', () => {
    // fc_start at 540 on a 300→420 ramp over 0..660: bt = 300 + 120*540/660.
    const cues = upcomingCues(REF, CURVE, 520, 45);
    expect(cues[0]!.btF).toBeCloseTo(300 + (120 * 540) / 660, 1);
  });

  it('omits btF when the marker time is outside the curve coverage', () => {
    const shortCurve: CurvePoint[] = [
      { t: 0, bt: 300 },
      { t: 100, bt: 340 },
    ];
    // dry_end at 300 is beyond the 0..100 curve → btF undefined.
    const cues = upcomingCues(REF, shortCurve, 256, 45);
    expect(kinds(cues)).toEqual(['dry_end']);
    expect(cues[0]!.btF).toBeUndefined();
  });

  it('handles an empty curve (no btF, cues still computed)', () => {
    const cues = upcomingCues(REF, [], 520, 45);
    expect(kinds(cues)).toEqual(['fc_start']);
    expect(cues[0]!.btF).toBeUndefined();
  });
});

describe('formatCue', () => {
  const cue = (over: Partial<ReferenceCue>): ReferenceCue => ({
    kind: 'fc_start',
    label: 'First crack',
    atT: 540,
    secondsUntil: 30,
    passed: false,
    terminal: false,
    ...over,
  });

  it('phrases an upcoming cue with a rounded countdown', () => {
    expect(formatCue(cue({ secondsUntil: 30 }))).toBe('First crack on the reference in ~30s');
    expect(formatCue(cue({ secondsUntil: 12.4 }))).toBe('First crack on the reference in ~12s');
  });

  it('phrases a due cue as "now"', () => {
    expect(formatCue(cue({ secondsUntil: 0, passed: true }))).toBe(
      'First crack on the reference now',
    );
  });

  it('phrases the terminal cue with elapsed-over seconds', () => {
    expect(
      formatCue(cue({ kind: 'drop', label: 'Drop', terminal: true, passed: true, secondsUntil: -40 })),
    ).toBe("Past the reference's drop by 40s");
    expect(
      formatCue(cue({ kind: 'drop', label: 'Drop', terminal: true, passed: true, secondsUntil: 0 })),
    ).toBe("At the reference's drop");
  });
});

describe('referenceEventLabel', () => {
  it('maps known kinds to human labels', () => {
    expect(referenceEventLabel('fc_start')).toBe('First crack');
    expect(referenceEventLabel('dry_end')).toBe('Dry end');
    expect(referenceEventLabel('drop')).toBe('Drop');
    expect(referenceEventLabel('turning_point')).toBe('Turning point');
  });

  it('falls back to the raw kind for unmapped events', () => {
    expect(referenceEventLabel('note')).toBe('note');
  });
});

describe('downsampleCurve', () => {
  const makeCurve = (n: number): CurvePoint[] =>
    Array.from({ length: n }, (_, i) => ({ t: i, bt: 300 + i }));

  it('returns a copy unchanged when already within the cap', () => {
    const c = makeCurve(500);
    const out = downsampleCurve(c, 600);
    expect(out).toHaveLength(500);
    expect(out).toEqual(c);
    expect(out).not.toBe(c); // fresh array, no mutation
  });

  it('reduces to at most maxPoints and preserves both endpoints', () => {
    const c = makeCurve(2000);
    const out = downsampleCurve(c, 600);
    expect(out.length).toBeLessThanOrEqual(600);
    expect(out.length).toBeGreaterThan(1);
    expect(out[0]).toEqual(c[0]);
    expect(out[out.length - 1]).toEqual(c[c.length - 1]);
  });

  it('keeps t strictly increasing (no duplicate indices)', () => {
    const out = downsampleCurve(makeCurve(5000), 600);
    for (let i = 1; i < out.length; i++) {
      expect(out[i]!.t).toBeGreaterThan(out[i - 1]!.t);
    }
  });

  it('defaults maxPoints to REFERENCE_CURVE_MAX_POINTS (600)', () => {
    expect(REFERENCE_CURVE_MAX_POINTS).toBe(600);
    expect(downsampleCurve(makeCurve(3000)).length).toBeLessThanOrEqual(600);
  });

  it('clamps a degenerate cap to keep endpoints', () => {
    const c = makeCurve(10);
    const out = downsampleCurve(c, 1);
    expect(out).toHaveLength(2);
    expect(out[0]).toEqual(c[0]);
    expect(out[1]).toEqual(c[9]);
  });

  it('handles tiny inputs without loss', () => {
    expect(downsampleCurve([], 600)).toEqual([]);
    const one = makeCurve(1);
    expect(downsampleCurve(one, 600)).toEqual(one);
  });
});
