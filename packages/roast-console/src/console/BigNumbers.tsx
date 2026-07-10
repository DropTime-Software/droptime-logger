'use client';

/**
 * BigNumbers — the glanceable-from-2-meters telemetry band.
 * BT is the enormous number (forest, matching the BT curve); ET (grass),
 * RoR (coral, always °F/min) and roast clock sit secondary; phase / DTR /
 * projected-drop / target-delta live as quiet chips underneath. All digits
 * are tabular-nums. Light theme, matching the web app's white card.
 */

import { CrosshairSimple } from '@phosphor-icons/react';
import type { ReactNode } from 'react';
import type { DropProjection, LiveSample, PhaseBreakdown, RoastPhase } from '../types';
import { tokens } from '../tokens';
import { fmtTemp, fmtTempDelta, mmss, splitFixed, tempSuffix, toDisplayTemp, type TempUnit } from '../chart/format';

export interface BigNumbersProps {
  sample?: LiveSample | null;
  /** live trailing RoR, °F/min */
  ror?: number;
  /** seconds-from-charge (negative during preheat) */
  elapsedT?: number;
  phase: PhaseBreakdown;
  projection?: DropProjection | null;
  /** current BT − target BT at the same t, °F */
  targetDeltaF?: number | null;
  /** display unit for temperature text (values stay °F); default 'F' */
  unit?: TempUnit;
}

const PHASE_META: Record<RoastPhase, { label: string; color: string }> = {
  preheat: { label: 'Preheat', color: tokens.textDim },
  drying: { label: 'Drying', color: tokens.phaseDrying },
  maillard: { label: 'Maillard', color: tokens.phaseMaillard },
  development: { label: 'Development', color: tokens.phaseDevelopment },
  cooling: { label: 'Cooling', color: tokens.grass },
  done: { label: 'Done', color: tokens.textDim },
};

export function BigNumbers({
  sample,
  ror,
  elapsedT,
  phase,
  projection,
  targetDeltaF,
  unit = 'F',
}: BigNumbersProps) {
  const bt = sample?.btF;
  const et = sample?.etF;
  const btParts = bt !== undefined ? splitFixed(toDisplayTemp(bt, unit), 1) : null;
  const phaseMeta = PHASE_META[phase.phase];

  const deltaColor =
    targetDeltaF !== null && targetDeltaF !== undefined && Math.abs(targetDeltaF) <= 5
      ? tokens.good
      : tokens.warn;

  const projStyle =
    projection?.confidence === 'high'
      ? { borderColor: 'rgba(2,69,34,0.45)', color: tokens.forest, borderStyle: 'solid' as const }
      : projection?.confidence === 'medium'
        ? { borderColor: tokens.hairline, color: 'rgba(36,36,36,0.72)', borderStyle: 'solid' as const }
        : { borderColor: 'rgba(36,36,36,0.25)', color: tokens.textDim, borderStyle: 'dashed' as const };

  return (
    <div className="flex flex-wrap items-end gap-x-10 gap-y-5">
      {/* BT — the number you read from across the roastery */}
      <div className="min-w-0">
        <div
          className="text-[11px] font-semibold uppercase tracking-[0.14em]"
          style={{ color: tokens.textDim }}
        >
          Bean temp · {tempSuffix(unit)}
        </div>
        <div
          className="font-semibold leading-[0.92] tracking-[-0.03em] tabular-nums"
          style={{ color: tokens.bt, fontSize: 'clamp(64px, 10vw, 118px)' }}
        >
          {btParts ? (
            <>
              {btParts.whole}
              <span className="text-[0.36em] font-medium" style={{ opacity: 0.6 }}>
                .{btParts.frac}°
              </span>
            </>
          ) : (
            <span style={{ color: tokens.textFaint }}>—</span>
          )}
        </div>
      </div>

      {/* secondary telemetry — still legible at 24px+ */}
      <div className="flex flex-wrap items-end gap-x-8 gap-y-4 pb-1">
        <Metric
          label="Env temp"
          value={et !== undefined ? `${fmtTemp(et, unit)}°` : '—'}
          color={et !== undefined ? tokens.et : tokens.textFaint}
        />
        <Metric
          label="Rate of rise"
          value={ror !== undefined ? ror.toFixed(1) : '—'}
          suffix={ror !== undefined ? '°F/min' : undefined}
          color={ror !== undefined ? tokens.ror : tokens.textFaint}
        />
        <Metric
          label="Roast time"
          value={elapsedT !== undefined ? mmss(elapsedT) : '—'}
          color={elapsedT !== undefined ? tokens.text : tokens.textFaint}
        />
      </div>

      {/* status chips */}
      <div className="flex w-full flex-wrap items-center gap-2">
        <Chip borderColor={tokens.hairline} color={phaseMeta.color}>
          <span
            className="inline-block h-1.5 w-1.5 rounded-full"
            style={{ background: phaseMeta.color }}
          />
          {phaseMeta.label}
        </Chip>

        {phase.dtr !== undefined && (
          <Chip bg={tokens.lemon} borderColor="transparent" color={tokens.pine}>
            DTR {(phase.dtr * 100).toFixed(1)}%
          </Chip>
        )}

        {projection && (
          <Chip borderColor={projStyle.borderColor} color={projStyle.color} borderStyle={projStyle.borderStyle}>
            <CrosshairSimple size={13} weight="bold" />
            Drop ≈ {mmss(projection.projectedDropSec)} at {fmtTemp(projection.targetDropTempF, unit)}°
            <span className="normal-case" style={{ color: tokens.textFaint }}>
              {projection.confidence}
            </span>
          </Chip>
        )}

        {targetDeltaF !== null && targetDeltaF !== undefined && (
          <Chip borderColor={tokens.hairline} color={deltaColor}>
            {fmtTempDelta(targetDeltaF, unit)}° vs target
          </Chip>
        )}
      </div>
    </div>
  );
}

function Metric({
  label,
  value,
  suffix,
  color,
}: {
  label: string;
  value: string;
  suffix?: string;
  color: string;
}) {
  return (
    <div>
      <div
        className="text-[10px] font-semibold uppercase tracking-[0.14em]"
        style={{ color: tokens.textDim }}
      >
        {label}
      </div>
      <div className="flex items-baseline gap-1">
        <span
          className="text-4xl font-semibold leading-none tracking-[-0.02em] tabular-nums"
          style={{ color }}
        >
          {value}
        </span>
        {suffix && (
          <span className="text-xs font-medium" style={{ color: tokens.textDim }}>
            {suffix}
          </span>
        )}
      </div>
    </div>
  );
}

function Chip({
  children,
  color,
  borderColor,
  borderStyle = 'solid',
  bg,
}: {
  children: ReactNode;
  color: string;
  borderColor: string;
  borderStyle?: 'solid' | 'dashed';
  bg?: string;
}) {
  return (
    <span
      className="inline-flex h-8 items-center gap-1.5 rounded-full border px-3 text-xs font-semibold uppercase tracking-[0.08em] tabular-nums"
      style={{ color, borderColor, borderStyle, background: bg }}
    >
      {children}
    </span>
  );
}
