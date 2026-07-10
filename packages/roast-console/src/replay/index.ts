/**
 * replay/ — background-replay reference math (pure functions, no React).
 *
 * Owner: reference feature (CONTRACTS §8). Cue math lives in the shared
 * package so droptime-web / droptime-app can reuse it against the same
 * contract types.
 *
 * "Roast against a previous roast": given a reference roast's markers + curve
 * and the live chart-time `nowT` (seconds-from-charge), report the reference
 * events the roaster should anticipate, so the UI can count them down, fire a
 * one-shot cue, and align a ghost curve. Playback-aid only — nothing here
 * actuates the machine or auto-marks the live roast.
 */
import type { CurvePoint, RoastEventKind, RoastMarkersSec } from '../types';

/** Default look-ahead window for {@link upcomingCues} (seconds). */
export const REFERENCE_CUE_WINDOW_SEC = 45;

/** Default ceiling for {@link downsampleCurve} — enough for a smooth ghost. */
export const REFERENCE_CURVE_MAX_POINTS = 600;

/** An upcoming reference-roast event the live roaster should anticipate. */
export interface ReferenceCue {
  /** which reference marker this cue announces */
  kind: RoastEventKind;
  /** short human label for the event, e.g. "First crack" */
  label: string;
  /** seconds-from-charge at which the REFERENCE roast hit this marker */
  atT: number;
  /** seconds until due on the live clock (atT − nowT); negative = already passed */
  secondsUntil: number;
  /** true once the live clock has reached/passed the reference marker time */
  passed: boolean;
  /** true only for the single terminal "past the reference's drop" cue */
  terminal: boolean;
  /** reference BT at atT, when the reference curve covers that time (°F) */
  btF?: number;
}

interface RefEvent {
  kind: RoastEventKind;
  field: 'turningPointSec' | 'dryEndSec' | 'fcStartSec' | 'fcEndSec' | 'dropSec';
  label: string;
}

/**
 * The time-bearing reference markers, in chronological order. Charge is always
 * at t = 0 (the rebase pin) so it is never an "upcoming" cue; sc_start/sc_end
 * are not carried by RoastMarkersSec and so cannot be referenced.
 */
const REFERENCE_EVENTS: readonly RefEvent[] = [
  { kind: 'turning_point', field: 'turningPointSec', label: 'Turning point' },
  { kind: 'dry_end', field: 'dryEndSec', label: 'Dry end' },
  { kind: 'fc_start', field: 'fcStartSec', label: 'First crack' },
  { kind: 'fc_end', field: 'fcEndSec', label: 'First crack end' },
  { kind: 'drop', field: 'dropSec', label: 'Drop' },
];

/** Human label for a reference event kind (falls back to the raw kind). */
export function referenceEventLabel(kind: RoastEventKind): string {
  return REFERENCE_EVENTS.find((e) => e.kind === kind)?.label ?? kind;
}

function eventOrder(kind: RoastEventKind): number {
  const i = REFERENCE_EVENTS.findIndex((e) => e.kind === kind);
  return i === -1 ? REFERENCE_EVENTS.length : i;
}

/** Reference BT at time `t`, linearly interpolated; undefined outside coverage. */
function btAt(curve: CurvePoint[], t: number): number | undefined {
  const n = curve.length;
  if (n === 0) return undefined;
  const first = curve[0]!;
  const last = curve[n - 1]!;
  if (t < first.t || t > last.t) return undefined;
  if (t === first.t) return round1(first.bt);
  if (t === last.t) return round1(last.bt);
  for (let i = 1; i < n; i++) {
    const a = curve[i - 1]!;
    const b = curve[i]!;
    if (t >= a.t && t <= b.t) {
      if (b.t === a.t) return round1(a.bt);
      const f = (t - a.t) / (b.t - a.t);
      return round1(a.bt + f * (b.bt - a.bt));
    }
  }
  return undefined;
}

function round1(v: number): number {
  return Math.round(v * 10) / 10;
}

function makeCue(
  ev: Pick<RefEvent, 'kind' | 'label'>,
  atT: number,
  nowT: number,
  curve: CurvePoint[],
  terminal: boolean,
): ReferenceCue {
  const secondsUntil = atT - nowT;
  const btF = btAt(curve, atT);
  return {
    kind: ev.kind,
    label: ev.label,
    atT,
    secondsUntil,
    passed: secondsUntil <= 0,
    terminal,
    ...(btF !== undefined ? { btF } : {}),
  };
}

/**
 * Reference-roast cues relative to the live clock `nowT` (t = seconds-from-charge).
 *
 * Semantics:
 * - Before charge (`nowT < 0`) → `[]`; cues are only meaningful once charged.
 * - Missing markers are skipped.
 * - Returns markers whose `atT` falls in `[nowT, nowT + windowSec]`, soonest
 *   first (stable chronological tie-break), each with `secondsUntil`/`passed`
 *   so the UI can render a countdown and fire once.
 * - Reference shorter than the live roast: once `nowT` passes the reference's
 *   drop, returns a single terminal drop cue ("past the reference's drop").
 */
export function upcomingCues(
  referenceMarkers: RoastMarkersSec,
  referenceCurve: CurvePoint[],
  nowT: number,
  windowSec: number = REFERENCE_CUE_WINDOW_SEC,
): ReferenceCue[] {
  if (!Number.isFinite(nowT) || nowT < 0) return [];

  const dropSec = referenceMarkers.dropSec;
  if (typeof dropSec === 'number' && Number.isFinite(dropSec) && nowT > dropSec) {
    // The reference ran shorter than the live roast — one terminal cue.
    return [makeCue({ kind: 'drop', label: 'Drop' }, dropSec, nowT, referenceCurve, true)];
  }

  const cues: ReferenceCue[] = [];
  for (const ev of REFERENCE_EVENTS) {
    const atT = referenceMarkers[ev.field];
    if (typeof atT !== 'number' || !Number.isFinite(atT)) continue;
    const secondsUntil = atT - nowT;
    if (secondsUntil >= 0 && secondsUntil <= windowSec) {
      cues.push(makeCue(ev, atT, nowT, referenceCurve, false));
    }
  }
  cues.sort((a, b) => a.atT - b.atT || eventOrder(a.kind) - eventOrder(b.kind));
  return cues;
}

/**
 * A ready-to-display / ready-to-speak phrase for a cue, e.g.
 * "First crack on the reference in ~30s". Rounds the countdown for stability.
 */
export function formatCue(cue: ReferenceCue): string {
  if (cue.terminal) {
    const over = Math.max(0, Math.round(-cue.secondsUntil));
    return over > 0
      ? `Past the reference's drop by ${over}s`
      : `At the reference's drop`;
  }
  const s = Math.round(cue.secondsUntil);
  if (s <= 0) return `${cue.label} on the reference now`;
  return `${cue.label} on the reference in ~${s}s`;
}

/**
 * Downsample a seconds-from-charge curve to at most `maxPoints` points for a
 * lightweight ghost overlay + compact storage. Endpoints (charge and drop/end)
 * are always preserved; interior points are picked at an even stride, keeping
 * the curve monotone in `t` and shape-faithful. Pure — mutates nothing.
 */
export function downsampleCurve(
  points: CurvePoint[],
  maxPoints: number = REFERENCE_CURVE_MAX_POINTS,
): CurvePoint[] {
  const cap = Math.max(2, Math.floor(maxPoints));
  const n = points.length;
  if (n <= cap) return points.slice();

  const out: CurvePoint[] = [];
  let lastIdx = -1;
  for (let i = 0; i < cap; i++) {
    const idx = Math.round((i * (n - 1)) / (cap - 1));
    if (idx !== lastIdx) {
      out.push(points[idx]!);
      lastIdx = idx;
    }
  }
  return out;
}
