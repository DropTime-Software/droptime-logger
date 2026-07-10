/**
 * Roast detail as a real route (owner: librarian). Reached via
 * `actions.navigate('detail', { roastUuid })`; the roast arrives in
 * `state.navParams.roastUuid`.
 *
 * Full post-roast view: static curve chart with a COMPARE overlay (up to two
 * other finished roasts drawn as ghosts), the roast record (RoastSummaryCard),
 * inline marker editing (update_roast_markers), meta editing (coffee library
 * autocomplete + charge/drop weights + notes → update_roast), export, and a
 * confirmed hard-delete (delete_roast → back to history).
 */
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import {
  LiveRoastChart,
  RoastSummaryCard,
  mmss,
  type CurvePoint,
  type RoastMarkersSec,
} from '@droptime/roast-console';
import type { AppMode, MarkerPatch, RoastPatch, RoastSummaryDto } from '../bridge';
import { ipc } from '../bridge';
import { useSettings } from '../settings/SettingsProvider';
import { useSession } from '../state/SessionProvider';
import { markersFromSummary, pointsFromRoast } from '../state/roast';
import { Button, Field, TextInput } from '../components/ui';
import { ExportButtons } from '../features/export/ExportButtons';
import { Combobox } from '../features/library/Combobox';
import { ConfirmDialog } from '../features/library/ConfirmDialog';
import { useLibrary } from '../features/library/useLibrary';

interface RoastData {
  summary: RoastSummaryDto;
  points: CurvePoint[];
  markers: RoastMarkersSec;
}

/** The five editable markers (charge is not editable in v0.1.0). */
const MARKER_FIELDS: Array<{ field: keyof MarkerPatch & keyof RoastMarkersSec; label: string }> = [
  { field: 'turningPointSec', label: 'Turning point' },
  { field: 'dryEndSec', label: 'Dry end' },
  { field: 'fcStartSec', label: 'First crack' },
  { field: 'fcEndSec', label: 'FC end' },
  { field: 'dropSec', label: 'Drop' },
];

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

/** Accepts "m:ss" or a raw seconds value; returns null when blank/invalid. */
function parseTimeInput(raw: string): number | null {
  const s = raw.trim();
  if (!s) return null;
  if (s.includes(':')) {
    const parts = s.split(':');
    if (parts.length !== 2) return null;
    const m = Number(parts[0]);
    const sec = Number(parts[1]);
    if (!Number.isFinite(m) || !Number.isFinite(sec)) return null;
    return m * 60 + sec;
  }
  const v = Number(s);
  return Number.isFinite(v) ? v : null;
}

function numOrNull(raw: string): number | null {
  const s = raw.trim();
  if (!s) return null;
  const v = Number(s);
  return Number.isFinite(v) ? v : null;
}

/**
 * The chart exposes a single `ghost` overlay, so multiple comparison curves are
 * threaded through it as one array. Every curve after the first is reversed so
 * the join hops drop→drop (a short, faint segment) rather than drop→charge (a
 * long diagonal). All bt values stay real, so the chart's extent scan is
 * unaffected.
 */
function combineGhosts(curves: CurvePoint[][]): CurvePoint[] | null {
  const usable = curves.filter((c) => c.length > 1);
  if (usable.length === 0) return null;
  if (usable.length === 1) return usable[0] ?? null;
  const out: CurvePoint[] = [...(usable[0] ?? [])];
  for (let i = 1; i < usable.length; i++) {
    const c = usable[i];
    if (c) out.push(...[...c].reverse());
  }
  return out;
}

export function DetailScreen({ mode }: { mode: AppMode }) {
  const { unit } = useSettings();
  const { state, actions } = useSession();
  const roastUuid = state.navParams?.roastUuid;

  if (mode !== 'tauri') {
    return (
      <Shell onBack={() => actions.navigate('history')} title="Roast detail">
        <div className="dt-card p-8 text-center">
          <p className="text-ink/70">Roast detail is desktop-only.</p>
          <p className="mt-1 text-sm text-ink/50">
            The browser demo doesn't save roasts. Install the desktop app to revisit and edit
            every roast.
          </p>
        </div>
      </Shell>
    );
  }

  if (!roastUuid) {
    return (
      <Shell onBack={() => actions.navigate('history')} title="Roast detail">
        <div className="dt-card p-8 text-center">
          <p className="text-ink/70">No roast selected.</p>
        </div>
      </Shell>
    );
  }

  return (
    <DetailBody
      key={roastUuid}
      roastUuid={roastUuid}
      unit={unit}
      onBack={() => actions.navigate('history')}
    />
  );
}

function Shell({
  title,
  onBack,
  children,
}: {
  title: string;
  onBack: () => void;
  children: React.ReactNode;
}) {
  return (
    <div className="h-full w-full overflow-y-auto">
      <div className="mx-auto flex min-h-full w-full max-w-[1600px] flex-col gap-6 px-6 py-8 lg:px-10">
        <div className="flex items-center justify-between">
          <button onClick={onBack} className="dt-focus-ring text-sm text-ink/60 hover:text-pine">
            ← Back to history
          </button>
          <h1 className="text-lg font-bold tracking-[-0.04em] text-pine">{title}</h1>
        </div>
        {children}
      </div>
    </div>
  );
}

function DetailBody({
  roastUuid,
  unit,
  onBack,
}: {
  roastUuid: string;
  unit: 'F' | 'C';
  onBack: () => void;
}) {
  const lib = useLibrary();
  const [data, setData] = useState<RoastData | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  // meta form
  const [coffeeName, setCoffeeName] = useState('');
  const [coffeeLocalId, setCoffeeLocalId] = useState<number | undefined>();
  const [chargeWeight, setChargeWeight] = useState('');
  const [dropWeight, setDropWeight] = useState('');
  const [notes, setNotes] = useState('');
  const [savingMeta, setSavingMeta] = useState(false);
  const [metaMsg, setMetaMsg] = useState<string | null>(null);

  // marker editing
  const [editingMarker, setEditingMarker] = useState<string | null>(null);
  const [markerDraft, setMarkerDraft] = useState('');
  const [markerBusy, setMarkerBusy] = useState(false);
  const [markerError, setMarkerError] = useState<string | null>(null);

  // compare
  const [candidates, setCandidates] = useState<RoastSummaryDto[]>([]);
  const [compareSel, setCompareSel] = useState<string[]>([]);
  const [compareCurves, setCompareCurves] = useState<Record<string, CurvePoint[]>>({});

  // delete
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);

  const chartRef = useRef<HTMLDivElement>(null);
  const [chartHeight, setChartHeight] = useState(440);

  const seedMetaForm = useCallback((s: RoastSummaryDto) => {
    setCoffeeName(s.coffeeName ?? '');
    setCoffeeLocalId(undefined);
    setChargeWeight(s.chargeWeightLb != null ? String(s.chargeWeightLb) : '');
    setDropWeight(s.dropWeightLb != null ? String(s.dropWeightLb) : '');
    setNotes(s.notes ?? '');
  }, []);

  const fetchData = useCallback(async (): Promise<RoastData> => {
    const { summary, samples, events } = await ipc.getRoast({ roastUuid });
    return { summary, points: pointsFromRoast(samples, events), markers: markersFromSummary(summary) };
  }, [roastUuid]);

  // initial load
  useEffect(() => {
    let cancelled = false;
    fetchData()
      .then((d) => {
        if (cancelled) return;
        setData(d);
        seedMetaForm(d.summary);
      })
      .catch((err) => {
        if (!cancelled) setLoadError(err instanceof Error ? err.message : 'Could not open the roast.');
      });
    return () => {
      cancelled = true;
    };
  }, [fetchData, seedMetaForm]);

  // compare candidates
  useEffect(() => {
    let cancelled = false;
    ipc
      .listRoasts()
      .then((list) => {
        if (cancelled) return;
        setCandidates(list.filter((r) => r.roastUuid !== roastUuid && r.status === 'finished'));
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [roastUuid]);

  useLayoutEffect(() => {
    const el = chartRef.current;
    if (!el) return;
    const ro = new ResizeObserver((entries) => {
      const h = entries[0]?.contentRect.height;
      if (h && h > 0) setChartHeight(Math.round(h));
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, [data]);

  const ghost = useMemo(
    () => combineGhosts(compareSel.map((u) => compareCurves[u]).filter((c): c is CurvePoint[] => !!c)),
    [compareSel, compareCurves],
  );

  async function toggleCompare(uuid: string) {
    if (compareSel.includes(uuid)) {
      setCompareSel((p) => p.filter((u) => u !== uuid));
      return;
    }
    if (compareSel.length >= 2) return;
    setCompareSel((p) => (p.includes(uuid) || p.length >= 2 ? p : [...p, uuid]));
    if (!compareCurves[uuid]) {
      try {
        const res = await ipc.getRoast({ roastUuid: uuid });
        setCompareCurves((prev) => ({ ...prev, [uuid]: pointsFromRoast(res.samples, res.events) }));
      } catch {
        // best-effort; the toggle just won't add a ghost
      }
    }
  }

  async function saveMeta() {
    if (!data) return;
    const s = data.summary;
    const patch: RoastPatch = {};

    const origCoffee = s.coffeeName ?? '';
    if (coffeeLocalId != null) {
      // picked a library coffee — backend denormalizes the name
      patch.coffeeLocalId = coffeeLocalId;
      patch.coffeeName = coffeeName.trim() || null;
    } else if (coffeeName.trim() !== origCoffee.trim()) {
      // free-text change — clear any link, keep the text
      patch.coffeeName = coffeeName.trim() || null;
      patch.coffeeLocalId = null;
    }

    const origCharge = s.chargeWeightLb != null ? String(s.chargeWeightLb) : '';
    if (chargeWeight.trim() !== origCharge) patch.chargeWeightLb = numOrNull(chargeWeight);
    const origDrop = s.dropWeightLb != null ? String(s.dropWeightLb) : '';
    if (dropWeight.trim() !== origDrop) patch.dropWeightLb = numOrNull(dropWeight);
    if (notes !== (s.notes ?? '')) patch.notes = notes.trim() || null;

    if (Object.keys(patch).length === 0) {
      setMetaMsg('No changes to save.');
      return;
    }
    setSavingMeta(true);
    setMetaMsg(null);
    try {
      const summary = await ipc.updateRoast({ roastUuid, patch });
      setData((prev) =>
        prev ? { summary, points: prev.points, markers: markersFromSummary(summary) } : prev,
      );
      seedMetaForm(summary);
      setMetaMsg('Saved.');
    } catch (err) {
      setMetaMsg(err instanceof Error ? err.message : 'Could not save changes.');
    } finally {
      setSavingMeta(false);
    }
  }

  async function commitMarker(field: string, markers: MarkerPatch) {
    setMarkerBusy(true);
    setMarkerError(null);
    try {
      await ipc.updateRoastMarkers({ roastUuid, markers });
      const fresh = await fetchData();
      setData(fresh);
      setEditingMarker(null);
    } catch (err) {
      setMarkerError(err instanceof Error ? err.message : 'Could not edit the marker.');
    } finally {
      setMarkerBusy(false);
    }
  }

  function startEdit(field: string, current?: number) {
    setEditingMarker(field);
    setMarkerError(null);
    setMarkerDraft(current != null ? mmss(current) : '');
  }

  async function saveMarker(field: string) {
    const secs = parseTimeInput(markerDraft);
    if (secs == null) {
      setMarkerError('Enter a time like 5:30 or a number of seconds.');
      return;
    }
    await commitMarker(field, { [field]: secs } as MarkerPatch);
  }

  async function doDelete() {
    setDeleting(true);
    setDeleteError(null);
    try {
      await ipc.deleteRoast({ roastUuid });
      onBack();
    } catch (err) {
      setDeleteError(err instanceof Error ? err.message : 'Could not delete the roast.');
      setDeleting(false);
    }
  }

  if (loadError) {
    return (
      <Shell onBack={onBack} title="Roast detail">
        <p className="text-sm text-red-600">{loadError}</p>
      </Shell>
    );
  }
  if (!data) {
    return (
      <Shell onBack={onBack} title="Roast detail">
        <p className="text-sm text-ink/50">Loading roast…</p>
      </Shell>
    );
  }

  const { summary, points, markers } = data;

  return (
    <div className="h-full w-full overflow-y-auto">
      <div className="mx-auto flex min-h-full w-full max-w-[1600px] flex-col gap-5 px-6 py-6 lg:px-10">
        {/* header */}
        <div className="flex flex-wrap items-center justify-between gap-3">
          <button onClick={onBack} className="dt-focus-ring text-sm text-ink/60 hover:text-pine">
            ← Back to history
          </button>
          <div className="flex items-center gap-3">
            <span className="text-sm text-ink/50">{fmtDate(summary.startedWallMs)}</span>
            <Button variant="danger" onClick={() => setConfirmDelete(true)}>
              Delete roast
            </Button>
          </div>
        </div>

        <div>
          <h1 className="text-2xl font-bold tracking-[-0.06em] text-pine">
            {summary.coffeeName ?? 'Untitled roast'}
          </h1>
          <p className="mt-1 text-sm text-ink/60">
            {summary.machineName ? `${summary.machineName} · ` : ''}
            {summary.status === 'finished' ? 'Finished' : 'Abandoned'}
            {summary.dropSec != null ? ` · ${mmss(summary.dropSec)} to drop` : ''}
          </p>
        </div>

        {/* chart + compare */}
        <div className="dt-card p-4 lg:p-5">
          <div ref={chartRef} className="min-h-[46vh]">
            <LiveRoastChart
              points={points}
              markers={markers}
              ghost={ghost}
              charged
              unit={unit}
              height={chartHeight}
              showRoR
              showEt
              live={false}
            />
          </div>

          {candidates.length > 0 ? (
            <div className="mt-3 border-t border-line pt-3">
              <div className="flex flex-wrap items-center gap-2">
                <span className="text-xs font-semibold uppercase tracking-wider text-ink/45">
                  Compare
                </span>
                <span className="text-xs text-ink/45">
                  overlay up to two other roasts as gray ghosts
                </span>
              </div>
              <div className="mt-2 flex flex-wrap gap-2">
                {candidates.map((c) => {
                  const active = compareSel.includes(c.roastUuid);
                  const disabled = !active && compareSel.length >= 2;
                  return (
                    <button
                      key={c.roastUuid}
                      onClick={() => void toggleCompare(c.roastUuid)}
                      disabled={disabled}
                      className="dt-focus-ring rounded-full border px-3 py-1.5 text-xs transition disabled:cursor-not-allowed disabled:opacity-40"
                      style={{
                        borderColor: active ? 'var(--color-forest)' : 'var(--color-line2)',
                        background: active ? 'rgba(218,246,152,0.35)' : '#ffffff',
                        color: active ? 'var(--color-forest)' : 'var(--color-ink)',
                      }}
                    >
                      {(c.coffeeName ?? 'Roast')} · {fmtDate(c.startedWallMs)}
                    </button>
                  );
                })}
              </div>
            </div>
          ) : null}
        </div>

        {/* record + edit rail */}
        <div className="grid grid-cols-1 gap-5 xl:grid-cols-[minmax(0,1fr)_400px]">
          <div className="min-w-0">
            <RoastSummaryCard summary={summary} points={points} unit={unit} />
          </div>

          <div className="flex flex-col gap-4">
            {/* markers editor */}
            <div className="dt-card flex flex-col gap-3 p-5">
              <h2 className="text-sm font-bold tracking-[-0.02em] text-pine">Markers</h2>
              <p className="text-xs leading-relaxed text-ink/50">
                Fix a mis-tapped marker — times are seconds from charge. Charge isn't editable.
              </p>
              {markerError ? <p className="text-xs text-red-600">{markerError}</p> : null}
              <div className="flex flex-col divide-y divide-line">
                {MARKER_FIELDS.map(({ field, label }) => {
                  const value = markers[field];
                  const editing = editingMarker === field;
                  return (
                    <div key={field} className="flex items-center gap-2 py-2">
                      <span className="w-28 shrink-0 text-sm text-ink/70">{label}</span>
                      {editing ? (
                        <>
                          <input
                            className="dt-input dt-focus-ring h-9 flex-1"
                            value={markerDraft}
                            autoFocus
                            placeholder="m:ss"
                            onChange={(e) => setMarkerDraft(e.target.value)}
                            onKeyDown={(e) => {
                              if (e.key === 'Enter') void saveMarker(field);
                              if (e.key === 'Escape') setEditingMarker(null);
                            }}
                          />
                          <button
                            className="dt-focus-ring text-xs font-semibold text-forest hover:underline disabled:opacity-40"
                            onClick={() => void saveMarker(field)}
                            disabled={markerBusy}
                          >
                            Save
                          </button>
                          <button
                            className="dt-focus-ring text-xs text-ink/50 hover:text-ink"
                            onClick={() => setEditingMarker(null)}
                            disabled={markerBusy}
                          >
                            Cancel
                          </button>
                        </>
                      ) : (
                        <>
                          <span className="flex-1 text-sm tabular-nums text-pine">
                            {value != null ? mmss(value) : <span className="text-ink/35">—</span>}
                          </span>
                          <button
                            className="dt-focus-ring text-xs font-semibold text-forest hover:underline"
                            onClick={() => startEdit(field, value)}
                          >
                            {value != null ? 'Edit' : 'Set'}
                          </button>
                          {value != null ? (
                            <button
                              className="dt-focus-ring text-xs text-ink/45 hover:text-red-600 disabled:opacity-40"
                              onClick={() => void commitMarker(field, { [field]: null } as MarkerPatch)}
                              disabled={markerBusy}
                              title="Clear this marker"
                            >
                              Clear
                            </button>
                          ) : null}
                        </>
                      )}
                    </div>
                  );
                })}
              </div>
            </div>

            {/* meta editor */}
            <div className="dt-card flex flex-col gap-4 p-5">
              <h2 className="text-sm font-bold tracking-[-0.02em] text-pine">Details</h2>
              <Field label="Coffee" htmlFor="detail-coffee">
                <Combobox
                  id="detail-coffee"
                  value={coffeeName}
                  selectedId={coffeeLocalId}
                  items={lib.coffees}
                  loading={lib.loading}
                  placeholder="e.g. Ethiopia Guji"
                  onCreate={lib.tauri ? lib.createCoffee : undefined}
                  createLabel={(name) => `Add “${name}” to library`}
                  onChange={(value, id) => {
                    setCoffeeName(value);
                    setCoffeeLocalId(id);
                    setMetaMsg(null);
                  }}
                />
              </Field>
              <div className="grid grid-cols-2 gap-3">
                <Field label="Charge (lb)" htmlFor="detail-charge">
                  <TextInput
                    id="detail-charge"
                    type="number"
                    inputMode="decimal"
                    min="0"
                    step="0.01"
                    value={chargeWeight}
                    onChange={(e) => {
                      setChargeWeight(e.target.value);
                      setMetaMsg(null);
                    }}
                  />
                </Field>
                <Field label="Drop (lb)" htmlFor="detail-drop">
                  <TextInput
                    id="detail-drop"
                    type="number"
                    inputMode="decimal"
                    min="0"
                    step="0.01"
                    value={dropWeight}
                    onChange={(e) => {
                      setDropWeight(e.target.value);
                      setMetaMsg(null);
                    }}
                  />
                </Field>
              </div>
              <Field label="Notes" htmlFor="detail-notes">
                <TextInput
                  id="detail-notes"
                  value={notes}
                  placeholder="Tasting notes, adjustments…"
                  onChange={(e) => {
                    setNotes(e.target.value);
                    setMetaMsg(null);
                  }}
                />
              </Field>
              <div className="flex items-center justify-between gap-3">
                <span className="text-xs text-ink/50">{metaMsg}</span>
                <Button variant="primary" onClick={() => void saveMeta()} disabled={savingMeta}>
                  {savingMeta ? 'Saving…' : 'Save details'}
                </Button>
              </div>
            </div>

            {/* export */}
            <ExportButtons roastUuid={roastUuid} />
          </div>
        </div>

        {/* stats footnote (also on the card, surfaced for quick glance) */}
        <div className="flex flex-wrap gap-x-8 gap-y-2 px-1 text-sm text-ink/60">
          <span>
            Total time <b className="text-pine">{summary.dropSec != null ? mmss(summary.dropSec) : '—'}</b>
          </span>
          <span>
            DTR <b className="text-pine">{fmtPct(summary.dtr)}</b>
          </span>
          <span>
            Weight loss <b className="text-pine">{fmtPct(summary.weightLossPct)}</b>
          </span>
        </div>
      </div>

      {confirmDelete ? (
        <ConfirmDialog
          title="Delete this roast?"
          message="This permanently removes the roast and all of its samples. This can't be undone."
          confirmLabel="Delete roast"
          busy={deleting}
          error={deleteError}
          onConfirm={() => void doDelete()}
          onCancel={() => {
            setConfirmDelete(false);
            setDeleteError(null);
          }}
        />
      ) : null}
    </div>
  );
}
