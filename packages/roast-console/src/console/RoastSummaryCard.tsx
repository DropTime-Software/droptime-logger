'use client';

/**
 * RoastSummaryCard — the post-drop debrief. Final curve (the same
 * LiveRoastChart, non-live, so review-mode scrub/zoom come free), the marker
 * table, DTR and weight-loss stats, and a development-phase callout scored
 * against the 15–25% window. Everything sits on the app's white card with
 * hairline separations — hierarchy is scale, not boxes.
 */

import type { CurvePoint, RoastSummary } from '../types';
import { tokens } from '../tokens';
import { LiveRoastChart } from '../chart';
import { fmtTemp, mmss, tempSuffix, type TempUnit } from '../chart/format';

export interface RoastSummaryCardProps {
  summary: RoastSummary;
  points: CurvePoint[];
  /** display unit for temperature text (values stay °F); default 'F' */
  unit?: TempUnit;
}

export function RoastSummaryCard({ summary, points, unit = 'F' }: RoastSummaryCardProps) {
  // nearest-sample BT lookup for markers that don't carry their own temp
  const btAt = (t: number | undefined): number | undefined => {
    if (t === undefined) return undefined;
    let best: CurvePoint | undefined;
    let bd = Infinity;
    for (const p of points) {
      const d = Math.abs(p.t - t);
      if (d < bd) {
        bd = d;
        best = p;
      }
    }
    return bd <= 10 ? best?.bt : undefined;
  };

  const dtr =
    summary.dtr ??
    (summary.fcStartSec !== undefined && summary.dropSec !== undefined && summary.dropSec > 0
      ? Math.round(((summary.dropSec - summary.fcStartSec) / summary.dropSec) * 1000) / 1000
      : undefined);
  const weightLossPct =
    summary.weightLossPct ??
    (summary.chargeWeightLb !== undefined && summary.dropWeightLb !== undefined && summary.chargeWeightLb > 0
      ? Math.round(((summary.chargeWeightLb - summary.dropWeightLb) / summary.chargeWeightLb) * 1000) / 10
      : undefined);
  const devSec =
    summary.fcStartSec !== undefined && summary.dropSec !== undefined
      ? summary.dropSec - summary.fcStartSec
      : undefined;

  const started = new Date(summary.startedWallMs);
  const when = started.toLocaleString(undefined, {
    month: 'short',
    day: 'numeric',
    hour: 'numeric',
    minute: '2-digit',
  });

  const markerRows: Array<{ key: string; label: string; t: number | undefined; tempF: number | undefined }> = [
    { key: 'charge', label: 'Charge', t: 0, tempF: summary.chargeTempF ?? btAt(0) },
    { key: 'tp', label: 'Turning point', t: summary.turningPointSec, tempF: summary.turningPointTempF ?? btAt(summary.turningPointSec) },
    { key: 'dry', label: 'Dry end', t: summary.dryEndSec, tempF: btAt(summary.dryEndSec) },
    { key: 'fc', label: 'First crack', t: summary.fcStartSec, tempF: btAt(summary.fcStartSec) },
    { key: 'fce', label: 'First crack end', t: summary.fcEndSec, tempF: btAt(summary.fcEndSec) },
    { key: 'drop', label: 'Drop', t: summary.dropSec, tempF: summary.dropTempF ?? btAt(summary.dropSec) },
  ].filter((r) => r.t !== undefined);

  const devCallout =
    dtr !== undefined
      ? dtr < 0.15
        ? { text: 'short of the 15–25% window', color: tokens.warn }
        : dtr > 0.25
          ? { text: 'past the 15–25% window', color: tokens.warn }
          : { text: 'inside the 15–25% window', color: tokens.good }
      : undefined;

  return (
    <div className="rounded-2xl border p-5 sm:p-6" style={{ borderColor: tokens.hairline }}>
      {/* header */}
      <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
        <div className="min-w-0">
          <div className="truncate text-lg font-semibold tracking-[-0.01em]" style={{ color: tokens.text }}>
            {summary.coffeeName ?? 'Untitled roast'}
          </div>
          <div className="text-[11px]" style={{ color: tokens.textDim }}>
            {summary.machineName ? `${summary.machineName} · ` : ''}
            {when}
          </div>
        </div>
        {summary.status !== 'finished' && (
          <span
            className="rounded-full border px-2.5 py-0.5 text-[10px] font-semibold uppercase tracking-[0.1em]"
            style={{ borderColor: tokens.hairline, color: tokens.warn }}
          >
            {summary.status}
          </span>
        )}
      </div>

      {/* final curve */}
      <div className="mt-4">
        <LiveRoastChart
          points={points}
          markers={summary}
          charged
          live={false}
          height={230}
          showRoR
          unit={unit}
        />
      </div>

      {/* headline stats */}
      <div
        className="mt-5 flex flex-wrap gap-x-10 gap-y-4 border-t pt-4"
        style={{ borderColor: tokens.hairline }}
      >
        <Stat label="Total time" value={summary.dropSec !== undefined ? mmss(summary.dropSec) : '—'} />
        <Stat
          label={`Drop temp · ${tempSuffix(unit)}`}
          value={summary.dropTempF !== undefined ? `${fmtTemp(summary.dropTempF, unit, 1)}°` : '—'}
          color={summary.dropTempF !== undefined ? tokens.forest : undefined}
        />
        <Stat
          label="DTR"
          value={dtr !== undefined ? `${(dtr * 100).toFixed(1)}%` : '—'}
          color={dtr !== undefined ? tokens.forest : undefined}
        />
        <Stat
          label="Weight loss"
          value={weightLossPct !== undefined ? `${weightLossPct.toFixed(1)}%` : '—'}
          sub={
            summary.chargeWeightLb !== undefined && summary.dropWeightLb !== undefined
              ? `${summary.chargeWeightLb} → ${summary.dropWeightLb} lb`
              : summary.chargeWeightLb !== undefined
                ? `${summary.chargeWeightLb} lb in — add drop weight`
                : undefined
          }
        />
        <Stat label="Development" value={devSec !== undefined ? mmss(devSec) : '—'} />
      </div>

      {/* development callout */}
      {devSec !== undefined && dtr !== undefined && devCallout && (
        <div className="mt-3 flex items-center gap-2 text-[12px]" style={{ color: tokens.textDim }}>
          <span className="inline-block h-1.5 w-1.5 rounded-full" style={{ background: devCallout.color }} />
          <span>
            Development ran <span className="font-semibold tabular-nums" style={{ color: tokens.text }}>{mmss(devSec)}</span>
            {' — '}
            <span className="font-semibold tabular-nums" style={{ color: devCallout.color }}>
              {(dtr * 100).toFixed(1)}%
            </span>{' '}
            of the roast, {devCallout.text}.
          </span>
        </div>
      )}

      {/* marker table */}
      {markerRows.length > 0 && (
        <div className="mt-5 border-t pt-2" style={{ borderColor: tokens.hairline }}>
          <div className="flex items-baseline justify-between py-1.5 text-[10px] font-semibold uppercase tracking-[0.12em]" style={{ color: tokens.textFaint }}>
            <span>Marker</span>
            <span className="flex gap-8">
              <span className="w-12 text-right">Time</span>
              <span className="w-14 text-right">BT</span>
            </span>
          </div>
          {markerRows.map((r) => (
            <div
              key={r.key}
              className="flex items-baseline justify-between border-t py-2 text-sm"
              style={{ borderColor: tokens.hairline }}
            >
              <span style={{ color: tokens.textDim }}>{r.label}</span>
              <span className="flex gap-8 tabular-nums">
                <span className="w-12 text-right font-semibold" style={{ color: tokens.text }}>
                  {r.t !== undefined ? mmss(r.t) : '—'}
                </span>
                <span className="w-14 text-right" style={{ color: r.tempF !== undefined ? tokens.forest : tokens.textFaint }}>
                  {r.tempF !== undefined ? `${fmtTemp(r.tempF, unit, 1)}°` : '—'}
                </span>
              </span>
            </div>
          ))}
        </div>
      )}

      {/* notes */}
      {summary.notes && (
        <div className="mt-5 border-t pt-4" style={{ borderColor: tokens.hairline }}>
          <div className="text-[10px] font-semibold uppercase tracking-[0.12em]" style={{ color: tokens.textFaint }}>
            Notes
          </div>
          <p className="mt-1 whitespace-pre-wrap text-sm leading-relaxed" style={{ color: 'rgba(36,36,36,0.8)' }}>
            {summary.notes}
          </p>
        </div>
      )}
    </div>
  );
}

function Stat({
  label,
  value,
  sub,
  color,
}: {
  label: string;
  value: string;
  sub?: string;
  color?: string;
}) {
  return (
    <div>
      <div className="text-[10px] font-semibold uppercase tracking-[0.12em]" style={{ color: tokens.textDim }}>
        {label}
      </div>
      <div className="mt-0.5 text-2xl font-semibold leading-none tracking-[-0.02em] tabular-nums" style={{ color: color ?? tokens.text }}>
        {value}
      </div>
      {sub && (
        <div className="mt-1 text-[11px] tabular-nums" style={{ color: tokens.textFaint }}>
          {sub}
        </div>
      )}
    </div>
  );
}
