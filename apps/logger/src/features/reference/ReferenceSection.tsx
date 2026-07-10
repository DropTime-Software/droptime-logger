/**
 * features/reference — reference-roast picker for SetupScreen
 * (CONTRACTS §8, owner: reference). "Roast against a previous roast."
 *
 * Self-contained: reads/writes the selection via `useSession()`
 * (`state.reference` + `actions.setReference`) and loads candidates itself via
 * `ipc.listRoasts` / `ipc.getRoast`. SetupScreen already forwards
 * `state.reference?.roastUuid` into StartConfig, and SessionProvider persists it
 * at start_session — so a picked reference needs no further wiring. The reducer
 * keeps `state.reference` through SESSION_STARTED, so the ghost + cues survive
 * into the live screen.
 *
 * Browser mode has no persistence (no roasts to pick), so this renders nothing.
 */
import { useEffect, useMemo, useState } from 'react';
import { MagnifyingGlass, X } from '@phosphor-icons/react';
import { downsampleCurve, mmss, tokens, type RoastSummary } from '@droptime/roast-console';
import { ipc } from '../../bridge';
import { useSession } from '../../state/SessionProvider';
import { markersFromSummary, pointsFromRoast } from '../../state/roast';
import { TextInput } from '../../components/ui';

/** Candidate references are finished roasts with a charge→drop span. */
function isCandidate(r: RoastSummary): boolean {
  return r.status === 'finished' && r.dropSec != null;
}

function fmtDate(ms: number): string {
  try {
    return new Date(ms).toLocaleDateString(undefined, {
      month: 'short',
      day: 'numeric',
      year: 'numeric',
    });
  } catch {
    return '';
  }
}

function fmtDtr(dtr?: number): string {
  return dtr != null ? `${Math.round(dtr * 100)}%` : '—';
}

function referenceLabel(r: RoastSummary): string {
  const name = r.coffeeName?.trim() || 'Untitled roast';
  const date = fmtDate(r.startedWallMs);
  return date ? `${name} · ${date}` : name;
}

export function ReferenceSection() {
  const { state, actions } = useSession();
  const mode = state.mode;
  const reference = state.reference;

  const [roasts, setRoasts] = useState<RoastSummary[] | null>(null);
  const [query, setQuery] = useState('');
  const [loadingUuid, setLoadingUuid] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Load candidate roasts (Tauri only — browser has no persistence).
  useEffect(() => {
    if (mode !== 'tauri' || reference) return;
    let cancelled = false;
    (async () => {
      try {
        const list = await ipc.listRoasts();
        if (!cancelled) setRoasts(list.filter(isCandidate));
      } catch {
        if (!cancelled) {
          setRoasts([]);
          setError('Could not load past roasts.');
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [mode, reference]);

  const filtered = useMemo(() => {
    if (!roasts) return null;
    const q = query.trim().toLowerCase();
    if (!q) return roasts;
    return roasts.filter((r) => (r.coffeeName ?? '').toLowerCase().includes(q));
  }, [roasts, query]);

  async function pick(r: RoastSummary) {
    setLoadingUuid(r.roastUuid);
    setError(null);
    try {
      const { summary, samples, events } = await ipc.getRoast({ roastUuid: r.roastUuid });
      // Rebase to seconds-from-charge, keep the post-charge span, and thin for a
      // light ghost overlay + compact reference state.
      const rebased = pointsFromRoast(samples, events).filter((p) => p.t >= 0);
      actions.setReference({
        roastUuid: r.roastUuid,
        label: referenceLabel(r),
        curve: downsampleCurve(rebased),
        markers: markersFromSummary(summary),
      });
    } catch {
      setError('Could not load that roast.');
    } finally {
      setLoadingUuid(null);
    }
  }

  // Browser mode: no reference workflow.
  if (mode !== 'tauri') return null;

  return (
    <section className="flex flex-col gap-2">
      <div className="flex items-baseline justify-between gap-3">
        <div>
          <p className="dt-label">Roast against a previous roast</p>
          <p className="mt-0.5 text-xs text-ink/50">
            Overlay a prior roast as a ghost curve and hear a heads-up as each milestone approaches.
          </p>
        </div>
        {reference ? (
          <button
            type="button"
            className="dt-focus-ring text-xs font-semibold text-ink/50 hover:text-red-600"
            onClick={() => actions.setReference(null)}
          >
            Clear
          </button>
        ) : null}
      </div>

      {reference ? (
        <div className="flex w-fit max-w-full items-center gap-2 rounded-full bg-mint px-3 py-1.5 text-sm">
          <span
            aria-hidden
            className="inline-block h-[3px] w-5 shrink-0 rounded"
            style={{ backgroundColor: tokens.ghost }}
          />
          <span className="truncate font-semibold text-pine">{reference.label}</span>
          <span className="shrink-0 text-[11px] font-semibold uppercase tracking-wide text-ink/40">
            ghost
          </span>
          <button
            type="button"
            aria-label="Remove reference roast"
            className="dt-focus-ring shrink-0 text-ink/40 hover:text-ink/70"
            onClick={() => actions.setReference(null)}
          >
            <X size={14} weight="bold" />
          </button>
        </div>
      ) : (
        <>
          <div className="relative">
            <MagnifyingGlass
              size={16}
              className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-ink/35"
            />
            <TextInput
              aria-label="Search past roasts by coffee"
              placeholder="Search by coffee…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              style={{ paddingLeft: '2.25rem' }}
            />
          </div>

          <div className="max-h-56 overflow-y-auto rounded-2xl bg-mint/40 p-1">
            {filtered === null ? (
              <p className="px-3 py-6 text-center text-sm text-ink/40">Loading past roasts…</p>
            ) : filtered.length === 0 ? (
              <p className="px-3 py-6 text-center text-sm text-ink/40">
                {roasts && roasts.length === 0
                  ? 'No finished roasts yet — log or import one to compare against.'
                  : 'No roasts match that search.'}
              </p>
            ) : (
              filtered.map((r) => (
                <button
                  key={r.roastUuid}
                  type="button"
                  className="dt-focus-ring flex w-full items-center justify-between gap-3 rounded-xl px-3 py-2 text-left hover:bg-white"
                  onClick={() => void pick(r)}
                  disabled={loadingUuid !== null}
                >
                  <span className="min-w-0">
                    <span className="block truncate text-sm font-semibold text-pine">
                      {r.coffeeName?.trim() || 'Untitled roast'}
                    </span>
                    <span className="text-xs text-ink/45">{fmtDate(r.startedWallMs)}</span>
                  </span>
                  <span className="flex shrink-0 items-center gap-3 text-xs text-ink/55">
                    <span>{mmss(r.dropSec ?? 0)}</span>
                    <span>DTR {fmtDtr(r.dtr)}</span>
                    {loadingUuid === r.roastUuid ? (
                      <span className="text-ink/40">…</span>
                    ) : null}
                  </span>
                </button>
              ))
            )}
          </div>
        </>
      )}

      {error ? <p className="text-xs text-red-600">{error}</p> : null}
    </section>
  );
}
