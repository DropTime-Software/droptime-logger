/**
 * Roast history (owner: librarian). Searchable list of finished/abandoned
 * roasts; each row opens the real detail route. Empty state teaches the three
 * ways to get roasts in and mounts the import affordance.
 */
import { useCallback, useEffect, useMemo, useState } from 'react';
import { mmss } from '@droptime/roast-console';
import type { AppMode, RoastSummaryDto } from '../bridge';
import { ipc } from '../bridge';
import { useSession } from '../state/SessionProvider';
import { Button, TextInput } from '../components/ui';
import { ImportDropzone } from '../features/import/ImportDropzone';

function fmtPct(v?: number): string {
  return v != null ? `${(v * 100).toFixed(1)}%` : '—';
}

function fmtDate(ms: number): string {
  return new Date(ms).toLocaleString(undefined, {
    month: 'short',
    day: 'numeric',
    hour: 'numeric',
    minute: '2-digit',
  });
}

export function HistoryScreen({ mode }: { mode: AppMode }) {
  const { actions } = useSession();
  const [roasts, setRoasts] = useState<RoastSummaryDto[] | null>(mode === 'tauri' ? null : []);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState('');

  const reload = useCallback(() => {
    if (mode !== 'tauri') return;
    setError(null);
    setRoasts(null);
    ipc
      .listRoasts()
      .then(setRoasts)
      .catch((err) => {
        setError(err instanceof Error ? err.message : 'Could not load roasts.');
        setRoasts([]);
      });
  }, [mode]);

  useEffect(() => {
    reload();
  }, [reload]);

  const filtered = useMemo(() => {
    if (!roasts) return null;
    const q = query.trim().toLowerCase();
    if (!q) return roasts;
    return roasts.filter((r) => (r.coffeeName ?? '').toLowerCase().includes(q));
  }, [roasts, query]);

  if (mode !== 'tauri') {
    return (
      <div className="mx-auto flex h-full max-w-lg flex-col items-center justify-center gap-4 px-6 text-center">
        <div className="dt-card w-full p-8">
          <h1 className="text-xl font-bold tracking-[-0.04em] text-pine">History is desktop-only</h1>
          <p className="mt-2 text-sm leading-relaxed text-ink/60">
            You're in the browser demo, so roasts aren't saved. Install the Droptime Logger
            desktop app to capture and revisit every roast — fully local, no account needed.
          </p>
          <div className="mt-5 flex justify-center">
            <Button variant="primary" onClick={() => void actions.startAnother()}>
              Back to a new roast
            </Button>
          </div>
        </div>
      </div>
    );
  }

  const isEmpty = roasts !== null && roasts.length === 0;

  return (
    <div className="mx-auto flex h-full w-full max-w-[1600px] flex-col gap-4 overflow-y-auto px-6 py-8 lg:px-10">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h1 className="text-2xl font-bold tracking-[-0.06em] text-pine">Roast history</h1>
        <div className="flex items-center gap-2">
          {!isEmpty ? (
            <div className="w-56">
              <TextInput
                placeholder="Search by coffee…"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                aria-label="Search roasts by coffee"
              />
            </div>
          ) : null}
          <Button variant="primary" onClick={() => void actions.startAnother()}>
            New roast
          </Button>
        </div>
      </div>

      {error ? <p className="text-sm text-red-600">{error}</p> : null}

      {roasts === null ? (
        <p className="text-sm text-ink/50">Loading…</p>
      ) : isEmpty ? (
        <div className="dt-card mx-auto mt-6 w-full max-w-xl p-8 text-center">
          <h2 className="text-lg font-bold tracking-[-0.03em] text-pine">No roasts yet</h2>
          <p className="mx-auto mt-2 max-w-md text-sm leading-relaxed text-ink/60">
            Run the demo, connect a roaster, or import your Artisan history — every roast is saved
            locally and shows up here.
          </p>
          <div className="mt-5 flex flex-col items-center gap-3">
            <Button variant="primary" onClick={() => void actions.startAnother()}>
              Start a roast
            </Button>
            <ImportDropzone onImported={() => reload()} />
          </div>
        </div>
      ) : (
        <>
          <div className="dt-card overflow-hidden">
            <div className="overflow-x-auto">
              <table className="w-full min-w-[720px] text-sm">
                <thead>
                  <tr className="bg-mint text-left text-[11px] uppercase tracking-wider text-ink/60">
                    <th className="px-4 py-3 font-semibold">Date</th>
                    <th className="px-4 py-3 font-semibold">Coffee</th>
                    <th className="px-4 py-3 font-semibold">Machine</th>
                    <th className="px-4 py-3 text-right font-semibold">Duration</th>
                    <th className="px-4 py-3 text-right font-semibold">DTR</th>
                    <th className="px-4 py-3 text-right font-semibold">Wt. loss</th>
                  </tr>
                </thead>
                <tbody>
                  {filtered && filtered.length > 0 ? (
                    filtered.map((r) => (
                      <tr
                        key={r.roastUuid}
                        onClick={() => actions.navigate('detail', { roastUuid: r.roastUuid })}
                        className="cursor-pointer border-b border-gray-100 transition last:border-0 hover:bg-gray-50"
                      >
                        <td className="whitespace-nowrap px-4 py-3 text-ink/80">
                          {fmtDate(r.startedWallMs)}
                        </td>
                        <td className="px-4 py-3">
                          <span className="font-medium text-pine">{r.coffeeName ?? '—'}</span>
                          {r.status === 'abandoned' ? (
                            <span className="ml-2 rounded-full bg-[#fef3c7] px-2 py-0.5 text-[10px] font-semibold text-[#92400e]">
                              Abandoned
                            </span>
                          ) : null}
                        </td>
                        <td className="px-4 py-3 text-ink/70">{r.machineName ?? '—'}</td>
                        <td className="px-4 py-3 text-right tabular-nums text-ink/80">
                          {r.dropSec != null ? mmss(r.dropSec) : '—'}
                        </td>
                        <td className="px-4 py-3 text-right tabular-nums text-ink/80">{fmtPct(r.dtr)}</td>
                        <td className="px-4 py-3 text-right tabular-nums text-ink/80">
                          {fmtPct(r.weightLossPct)}
                        </td>
                      </tr>
                    ))
                  ) : (
                    <tr>
                      <td colSpan={6} className="px-4 py-8 text-center text-sm text-ink/50">
                        No roasts match “{query}”.
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
          </div>

          {/* Import affordance stays available once there are roasts too. */}
          <ImportDropzone onImported={() => reload()} />
        </>
      )}
    </div>
  );
}
