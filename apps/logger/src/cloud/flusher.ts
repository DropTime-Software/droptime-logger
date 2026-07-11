/**
 * The outbox drainer (build plan §10). Pulls pending roasts from the durable
 * SQLite outbox (store/sync.rs) and replays them through the Cloud `logger.*`
 * mutations in dependency order, then acks each accepted row.
 *
 * Ordering (enforced by server typed throws): upsertMachine → appendSampleChunk×N
 * → finalizeRoast for a `finalize` row; upsertMachine → importLocalHistory for an
 * `import` row. Every mutation is idempotent (by_org_client on clientRoastId), so
 * a row read-but-not-acked simply replays. We drain oldest-first, one row at a
 * time, and stop on the first gate/cap/transport error — leaving that row (and
 * everything after it) pending for the next pass.
 */
import type { ConvexHttpClient } from 'convex/browser';

import { ipc } from '../bridge';
import type { SyncRoast } from '../bridge/dto';
import {
  appendChunkArgs,
  chunkCurve,
  finalizeArgs,
  fns,
  importRow,
  upsertMachineArgs,
} from './mutations';

/** Backstops from getActiveOrg — pause the drainer and route the user. */
export type Gate = 'unauthenticated' | 'onboarding' | 'paywall';
/** Free-tier caps surfaced during sync — stop this row, prompt an upgrade. */
export type Cap = 'machine-limit' | 'roast-monthly-limit';

export interface FlushResult {
  synced: number;
  pending: number;
  gate?: Gate;
  cap?: Cap;
  error?: string;
}

function classify(err: unknown): { gate?: Gate; cap?: Cap; message: string } {
  const message = err instanceof Error ? err.message : String(err);
  if (message.includes('UNAUTHENTICATED')) return { gate: 'unauthenticated', message };
  if (message.includes('ONBOARDING_REQUIRED')) return { gate: 'onboarding', message };
  if (message.includes('PAYWALL_REQUIRED')) return { gate: 'paywall', message };
  if (message.includes('MACHINE_LIMIT_REACHED')) return { cap: 'machine-limit', message };
  if (message.includes('ROAST_MONTHLY_LIMIT_REACHED'))
    return { cap: 'roast-monthly-limit', message };
  return { message };
}

/** Replay ONE roast; returns the created batch id. Throws typed server errors. */
async function syncOne(http: ConvexHttpClient, r: SyncRoast): Promise<string> {
  await http.mutation(fns.upsertMachine, upsertMachineArgs(r));

  if (r.op === 'import') {
    const results = (await http.mutation(fns.importLocalHistory, {
      roasts: [importRow(r)],
    })) as Array<{ batchId: string }>;
    return results[0]?.batchId ?? '';
  }

  // finalize: push the curve as fixed windows, then finalize (server assembles).
  const chunks = chunkCurve(r.curve);
  for (const [i, chunk] of chunks.entries()) {
    await http.mutation(fns.appendSampleChunk, appendChunkArgs(r, i, chunk));
  }
  const res = (await http.mutation(fns.finalizeRoast, finalizeArgs(r))) as {
    batchId: string;
  };
  return res.batchId;
}

/**
 * Drain the outbox. Returns the count synced this pass, the remaining pending
 * count, and any gate/cap/error that halted it (so the UI can route the user).
 */
export async function flushOutbox(
  http: ConvexHttpClient,
  getToken: () => Promise<string | null>,
): Promise<FlushResult> {
  const pending = await ipc.syncPending(25);
  let synced = 0;

  for (const roast of pending) {
    // Set a fresh convex-template JWT before each roast (clerk-js caches ~60s
    // and refreshes near expiry, so this keeps every request authenticated).
    const token = await getToken();
    if (!token) {
      return {
        synced,
        pending: await ipc.syncPendingCount().catch(() => pending.length - synced),
        gate: 'unauthenticated',
        error: 'No Droptime auth token — sign in again.',
      };
    }
    http.setAuth(token);
    try {
      const batchId = await syncOne(http, roast);
      await ipc.syncMarkSynced([
        { outboxId: roast.outboxId, clientRoastId: roast.clientRoastId, batchId },
      ]);
      synced += 1;
    } catch (err) {
      const { gate, cap, message } = classify(err);
      return {
        synced,
        pending: (await ipc.syncPendingCount().catch(() => pending.length - synced)),
        gate,
        cap,
        error: message,
      };
    }
  }

  return { synced, pending: await ipc.syncPendingCount().catch(() => 0) };
}
