/**
 * The Cloud `logger.*` mutation surface + the pure mapping from a local
 * `SyncRoast` (assembled by store/sync.rs) onto each mutation's args.
 *
 * No codegen from droptime-app here — we reference the deployed functions by
 * string path via `makeFunctionReference`. Every mutation is idempotent
 * server-side (by_org_client on clientRoastId), so the drainer replays freely.
 */
import { makeFunctionReference } from 'convex/server';

import type { CurvePoint, SyncRoast } from '../bridge/dto';

export const fns = {
  upsertMachine: makeFunctionReference<'mutation'>('logger:upsertMachine'),
  appendSampleChunk: makeFunctionReference<'mutation'>('logger:appendSampleChunk'),
  finalizeRoast: makeFunctionReference<'mutation'>('logger:finalizeRoast'),
  importLocalHistory: makeFunctionReference<'mutation'>('logger:importLocalHistory'),
} as const;

/** convex/logger.ts MAX_CHUNK_SAMPLES. */
export const MAX_CHUNK_SAMPLES = 1200;

/** Strip `undefined` keys — Convex values may not be `undefined`. */
function defined<T extends Record<string, unknown>>(obj: T): Partial<T> {
  const out: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(obj)) {
    if (v !== undefined) out[k] = v;
  }
  return out as Partial<T>;
}

/** epoch-ms → ISO `YYYY-MM-DD` (the roast date the server records). */
export function isoDate(startedWallMs: number): string {
  return new Date(startedWallMs).toISOString().slice(0, 10);
}

/** Split a curve into fixed windows the server accepts (≤ MAX_CHUNK_SAMPLES). */
export function chunkCurve(curve: CurvePoint[], size = MAX_CHUNK_SAMPLES): CurvePoint[][] {
  if (curve.length === 0) return [];
  const chunks: CurvePoint[][] = [];
  for (let i = 0; i < curve.length; i += size) {
    chunks.push(curve.slice(i, i + size));
  }
  return chunks;
}

/** upsertMachine args — runs first so insertBatch has a real machine. */
export function upsertMachineArgs(r: SyncRoast) {
  return defined({
    clientMachineId: r.clientMachineId,
    name: r.machineName,
    make: r.machineMake,
    // Seed capacity from the charge weight; the server only ratchets up.
    batchCapacityLb: r.chargeWeightLb > 0 ? r.chargeWeightLb : undefined,
  });
}

/** The flattened canonical markers, shared by finalize + import. */
function markerArgs(r: SyncRoast) {
  return {
    chargeTempF: r.chargeTempF,
    turningPointSec: r.turningPointSec,
    turningPointTempF: r.turningPointTempF,
    dryEndSec: r.dryEndSec,
    fcStartSec: r.fcStartSec,
    fcEndSec: r.fcEndSec,
    dropSec: r.dropSec,
    dropTempF: r.dropTempF,
  };
}

/** appendSampleChunk args for one window (chunks pushed in ascending order). */
export function appendChunkArgs(r: SyncRoast, chunkIndex: number, samples: CurvePoint[]) {
  return { clientRoastId: r.clientRoastId, chunkIndex, samples };
}

/** finalizeRoast args — the live/single-roast path (readout + monthly count). */
export function finalizeArgs(r: SyncRoast) {
  return defined({
    clientRoastId: r.clientRoastId,
    clientMachineId: r.clientMachineId,
    deviceId: r.deviceId,
    coffeeName: r.coffeeName,
    roastDate: isoDate(r.startedWallMs),
    chargeWeightLb: r.chargeWeightLb,
    dropWeightLb: r.dropWeightLb,
    ...markerArgs(r),
  });
}

/** One importLocalHistory row — the bulk backfill path (curve inline, skipReadout). */
export function importRow(r: SyncRoast) {
  return defined({
    clientRoastId: r.clientRoastId,
    clientMachineId: r.clientMachineId,
    machineName: r.machineName,
    coffeeName: r.coffeeName,
    roastDate: isoDate(r.startedWallMs),
    chargeWeightLb: r.chargeWeightLb,
    dropWeightLb: r.dropWeightLb,
    ...markerArgs(r),
    curve: r.curve,
  });
}
