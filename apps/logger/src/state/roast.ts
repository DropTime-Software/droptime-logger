/**
 * Pure roast helpers shared by the store and the history screen. All math routes
 * through @droptime/roast-console so browser (in-process) and Tauri (backend) modes
 * stay in lock-step.
 */
import {
  computeDtr,
  detectTurningPoint,
  rebaseSamples,
  weightLossPct,
  type CurvePoint,
  type LiveSample,
  type RoastEvent,
  type RoastEventKind,
  type RoastMarkersSec,
  type RoastSummary,
} from '@droptime/roast-console';
import type { SessionMeta } from './types';

/** Expected mark order used by "mark next" (Space) and default flow. */
export const EXPECTED_ORDER: RoastEventKind[] = [
  'charge',
  'dry_end',
  'fc_start',
  'fc_end',
  'drop',
];

/** Keyboard single-key bindings (CONTRACTS brief §3). */
export const KEY_TO_KIND: Record<string, RoastEventKind> = {
  c: 'charge',
  d: 'dry_end',
  f: 'fc_start',
  g: 'fc_end',
  x: 'drop',
};

export function isMarked(markers: RoastMarkersSec, kind: RoastEventKind, charged: boolean): boolean {
  switch (kind) {
    case 'charge':
      return charged;
    case 'turning_point':
      return markers.turningPointSec != null;
    case 'dry_end':
      return markers.dryEndSec != null;
    case 'fc_start':
      return markers.fcStartSec != null;
    case 'fc_end':
      return markers.fcEndSec != null;
    case 'drop':
      return markers.dropSec != null;
    default:
      return false;
  }
}

/** First expected event not yet marked (undefined once the flow is complete). */
export function nextExpected(
  markers: RoastMarkersSec,
  charged: boolean,
): RoastEventKind | undefined {
  return EXPECTED_ORDER.find((k) => !isMarked(markers, k, charged));
}

/** Sample nearest to a target session-second (used to pin charge/drop temps). */
export function nearestSample(
  samples: readonly LiveSample[],
  sessionSec: number,
): LiveSample | undefined {
  let best: LiveSample | undefined;
  let bestDelta = Infinity;
  for (const s of samples) {
    const d = Math.abs(s.sessionSec - sessionSec);
    if (d < bestDelta) {
      bestDelta = d;
      best = s;
    }
  }
  return best;
}

/**
 * Apply a mark locally (browser mode). Mirrors the backend semantics in
 * CONTRACTS.md §2: charge pins chargeTempF from the nearest sample; drop pins
 * dropTempF; all *Sec markers are seconds-from-charge.
 */
export function applyBrowserMark(
  prev: RoastMarkersSec,
  kind: RoastEventKind,
  sessionSec: number,
  chargeSessionSec: number | undefined,
  near: LiveSample | undefined,
): RoastMarkersSec {
  const m: RoastMarkersSec = { ...prev };
  const rel = chargeSessionSec != null ? sessionSec - chargeSessionSec : undefined;
  switch (kind) {
    case 'charge':
      m.chargeTempF = near?.btF;
      break;
    case 'turning_point':
      m.turningPointSec = rel;
      m.turningPointTempF = near?.btF;
      break;
    case 'dry_end':
      m.dryEndSec = rel;
      break;
    case 'fc_start':
      m.fcStartSec = rel;
      break;
    case 'fc_end':
      m.fcEndSec = rel;
      break;
    case 'drop':
      m.dropSec = rel;
      m.dropTempF = near?.btF;
      break;
    default:
      break;
  }
  return m;
}

/** Clear a single marker locally (browser undo). */
export function clearBrowserMark(
  prev: RoastMarkersSec,
  kind: RoastEventKind,
): RoastMarkersSec {
  const m: RoastMarkersSec = { ...prev };
  switch (kind) {
    case 'charge':
      m.chargeTempF = undefined;
      break;
    case 'turning_point':
      m.turningPointSec = undefined;
      m.turningPointTempF = undefined;
      break;
    case 'dry_end':
      m.dryEndSec = undefined;
      break;
    case 'fc_start':
      m.fcStartSec = undefined;
      break;
    case 'fc_end':
      m.fcEndSec = undefined;
      break;
    case 'drop':
      m.dropSec = undefined;
      m.dropTempF = undefined;
      break;
    default:
      break;
  }
  return m;
}

/**
 * Assemble a RoastSummary from live state. Used for the browser-mode finalize and
 * for the Summary screen's live preview as the drop weight is typed. Fills the
 * turning point via detectTurningPoint when it was never marked (math parity with
 * finish_session).
 */
export function buildLiveSummary(args: {
  roastUuid: string;
  startedWallMs: number;
  meta: SessionMeta | undefined;
  markers: RoastMarkersSec;
  points: CurvePoint[];
  dropWeightLb?: number;
  notes?: string;
}): RoastSummary {
  const markers: RoastMarkersSec = { ...args.markers };
  if (markers.turningPointSec == null) {
    // Exclude the post-drop cool-down: TP is the pre-drop BT minimum, so a
    // cool-down dip must never win. Fall back to all points when drop is unknown.
    const cand =
      markers.dropSec != null ? args.points.filter((p) => p.t <= markers.dropSec!) : args.points;
    const tp = detectTurningPoint(cand);
    if (tp) {
      markers.turningPointSec = tp.t;
      markers.turningPointTempF = tp.bt;
    }
  }
  return {
    ...markers,
    roastUuid: args.roastUuid,
    status: 'finished',
    startedWallMs: args.startedWallMs,
    machineName: args.meta?.machineName,
    coffeeName: args.meta?.coffeeName,
    chargeWeightLb: args.meta?.chargeWeightLb,
    dropWeightLb: args.dropWeightLb,
    weightLossPct: weightLossPct(args.meta?.chargeWeightLb, args.dropWeightLb),
    dtr: computeDtr(markers.fcStartSec, markers.dropSec),
    notes: args.notes,
  };
}

/** Extract just the marker fields from a summary (for the history detail chart). */
export function markersFromSummary(summary: RoastSummary): RoastMarkersSec {
  return {
    turningPointSec: summary.turningPointSec,
    turningPointTempF: summary.turningPointTempF,
    dryEndSec: summary.dryEndSec,
    fcStartSec: summary.fcStartSec,
    fcEndSec: summary.fcEndSec,
    dropSec: summary.dropSec,
    dropTempF: summary.dropTempF,
    chargeTempF: summary.chargeTempF,
  };
}

/**
 * Rebase persisted samples into chart points. chargeSessionSec is recovered from
 * the raw charge event (samples/summary alone don't carry the absolute pin).
 */
export function pointsFromRoast(samples: LiveSample[], events: RoastEvent[]): CurvePoint[] {
  const charge = events.find((e) => e.kind === 'charge');
  return rebaseSamples(samples, charge?.sessionSec);
}

/** Reconstruct the mark-history stack (charge→drop order) from raw events. */
export function historyFromEvents(events: RoastEvent[]): RoastEventKind[] {
  return [...events]
    .filter((e) => EXPECTED_ORDER.includes(e.kind) || e.kind === 'turning_point')
    .sort((a, b) => a.sessionSec - b.sessionSec)
    .map((e) => e.kind);
}
