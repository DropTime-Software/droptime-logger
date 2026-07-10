import { useEffect, useMemo, useState } from 'react';
import { RoastSummaryCard, mmss } from '@droptime/roast-console';
import type { AppMode } from '../bridge';
import { useSettings } from '../settings/SettingsProvider';
import { useSession } from '../state/SessionProvider';
import { Button, Field, TextInput } from '../components/ui';
import { ExportButtons } from '../features/export/ExportButtons';

/** Post-drop context shown on the frozen summary chart, seconds. */
const DROP_MARGIN_SEC = 15;

/** 2Hz countdown for the cool-down chip. */
function useCountdown(endsWallMs: number | undefined): number | null {
  const [left, setLeft] = useState<number | null>(null);
  useEffect(() => {
    if (endsWallMs == null) {
      setLeft(null);
      return;
    }
    const tick = () => setLeft(Math.max(0, (endsWallMs - performance.now()) / 1000));
    tick();
    const id = window.setInterval(tick, 500);
    return () => window.clearInterval(id);
  }, [endsWallMs]);
  return left;
}

export function SummaryScreen({ mode }: { mode: AppMode }) {
  const { unit } = useSettings();
  const { state, derived, capturing, coolDown, actions } = useSession();
  const [dropWeight, setDropWeight] = useState('');
  const [notes, setNotes] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState<'another' | 'history' | null>(null);

  const dropWeightLb = dropWeight.trim() ? Number(dropWeight) : undefined;
  const validWeight = dropWeightLb != null && Number.isFinite(dropWeightLb) ? dropWeightLb : undefined;
  const finalized = !!state.finalSummary;
  const coolDownLeft = useCountdown(coolDown?.endsWallMs);

  // Live preview so weight-loss updates as the drop weight is typed.
  const summary = useMemo(
    () => actions.previewSummary(validWeight, notes.trim() || undefined),
    [actions, validWeight, notes, state.finalSummary, derived.points],
  );

  // The summary is a RECORD of the roast: freeze the chart at drop (+ a little
  // context). Cool-down samples keep persisting underneath; they just don't
  // grow this chart.
  const points = useMemo(() => {
    const ds = summary.dropSec;
    if (ds == null) return derived.points;
    return derived.points.filter((p) => p.t <= ds + DROP_MARGIN_SEC);
  }, [derived.points, summary.dropSec]);

  async function finishThen(next: 'another' | 'history') {
    setPending(next);
    setError(null);
    try {
      await actions.finishRoast(validWeight, notes.trim() || undefined);
      if (next === 'another') await actions.startAnother();
      else actions.navigate('history');
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Could not save the roast.');
      setPending(null);
    }
  }

  return (
    <div className="h-full w-full overflow-y-auto">
      <div className="mx-auto grid w-full max-w-[1600px] grid-cols-1 gap-6 px-6 py-8 lg:px-10 xl:grid-cols-[minmax(0,1fr)_380px]">
        {/* ---- left: the roast record ---- */}
        <div className="flex min-w-0 flex-col gap-5">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div>
              <h1 className="text-2xl font-bold tracking-[-0.06em] text-pine">Roast complete</h1>
              <p className="mt-1 text-sm text-ink/60">
                {mode === 'browser'
                  ? 'Browser demo — this summary is not saved.'
                  : finalized
                    ? 'Saved locally.'
                    : 'Record the drop weight, then save.'}
              </p>
            </div>
            {coolDown && capturing && coolDownLeft != null ? (
              <div className="flex items-center gap-2.5 rounded-full border border-line2 bg-[#fdf7ee] px-3.5 py-1.5 text-xs font-semibold text-ink/70">
                <span className="h-2 w-2 animate-pulse rounded-full bg-amber" />
                Recording cool-down · {mmss(coolDownLeft)} left
                <button
                  className="dt-focus-ring font-semibold text-forest underline-offset-2 hover:underline"
                  onClick={() => void actions.stopCaptureNow()}
                >
                  Stop log
                </button>
              </div>
            ) : null}
          </div>

          <RoastSummaryCard summary={summary} points={points} unit={unit} />
        </div>

        {/* ---- right: finish rail (sticky on wide screens) ---- */}
        <div className="flex flex-col gap-4 xl:sticky xl:top-8 xl:self-start">
          <div className="dt-card flex flex-col gap-4 p-5">
            <Field label="Drop weight (lb)" htmlFor="dropw" hint="Sets weight loss %.">
              <TextInput
                id="dropw"
                type="number"
                inputMode="decimal"
                min="0"
                step="0.01"
                placeholder="e.g. 1.68"
                value={dropWeight}
                disabled={finalized}
                onChange={(e) => setDropWeight(e.target.value)}
              />
            </Field>
            <Field label="Notes" htmlFor="notes">
              <TextInput
                id="notes"
                placeholder="Tasting notes, adjustments for next time…"
                value={notes}
                disabled={finalized}
                onChange={(e) => setNotes(e.target.value)}
              />
            </Field>

            {error ? <p className="text-sm text-red-600">{error}</p> : null}

            <div className="flex flex-col gap-2.5 pt-1">
              <Button
                variant="primary"
                onClick={() => void finishThen('another')}
                disabled={pending !== null}
              >
                {pending === 'another' ? 'Saving…' : 'Start another roast'}
              </Button>
              <Button
                variant="ghost"
                onClick={() => void finishThen('history')}
                disabled={pending !== null}
              >
                {pending === 'history' ? 'Saving…' : 'View history'}
              </Button>
            </div>
          </div>

          {/* Export actions (feature mount point — renders null until it lands). */}
          <ExportButtons roastUuid={state.roastUuid} />

          <p className="px-1 text-xs leading-relaxed text-ink/45">
            Capture runs for one minute after drop so the cool-down is kept and an
            accidental drop can be undone — then the log closes on its own.
          </p>
        </div>
      </div>
    </div>
  );
}
