'use client';

/**
 * PhaseBars — one horizontal instrument strip: drying / Maillard / development
 * as proportional segments of time-since-charge, live percentages beneath,
 * the current phase pulsing gently. Coral is reserved for RoR, so Maillard
 * reads copper here. Before charge the track sits empty and says so.
 */

import type { PhaseBreakdown } from '../types';
import { tokens } from '../tokens';
import { RcKeyframes } from '../chart/Keyframes';

export interface PhaseBarsProps {
  phase: PhaseBreakdown;
}

const SEGMENTS = [
  { key: 'drying', label: 'Drying', color: tokens.phaseDrying },
  { key: 'maillard', label: 'Maillard', color: tokens.phaseMaillard },
  { key: 'development', label: 'Development', color: tokens.phaseDevelopment },
] as const;

export function PhaseBars({ phase }: PhaseBarsProps) {
  const pctOf = (key: (typeof SEGMENTS)[number]['key']): number | undefined =>
    key === 'drying' ? phase.dryingPct : key === 'maillard' ? phase.maillardPct : phase.developmentPct;

  const hasData = SEGMENTS.some((s) => (pctOf(s.key) ?? 0) > 0);

  return (
    <div>
      <RcKeyframes />
      {/* the strip */}
      <div
        className="flex h-1.5 w-full overflow-hidden rounded-full"
        style={{ background: 'rgba(2,69,34,0.08)' }}
      >
        {hasData &&
          SEGMENTS.map((s) => {
            const pct = pctOf(s.key);
            if (pct === undefined || pct <= 0) return null;
            const active = phase.phase === s.key;
            return (
              <div
                key={s.key}
                className={active ? 'rc-anim' : undefined}
                style={{
                  width: `${Math.min(100, Math.max(0, pct))}%`,
                  background: s.color,
                  animation: active ? 'rc-pulse 2.6s ease-in-out infinite' : undefined,
                }}
              />
            );
          })}
      </div>

      {/* live percentages */}
      <div className="mt-2 flex flex-wrap items-baseline gap-x-5 gap-y-1">
        {hasData ? (
          SEGMENTS.map((s) => {
            const pct = pctOf(s.key);
            const active = phase.phase === s.key;
            const shown = pct !== undefined && pct > 0;
            return (
              <span key={s.key} className="inline-flex items-baseline gap-1.5 text-[11px]">
                <span
                  className="inline-block h-1.5 w-1.5 self-center rounded-full"
                  style={{ background: shown ? s.color : tokens.textFaint }}
                />
                <span
                  className={active ? 'font-semibold uppercase tracking-[0.08em]' : 'uppercase tracking-[0.08em]'}
                  style={{ color: active ? tokens.text : tokens.textDim }}
                >
                  {s.label}
                </span>
                <span
                  className="font-semibold tabular-nums"
                  style={{ color: shown ? (active ? s.color : tokens.textDim) : tokens.textFaint }}
                >
                  {shown ? `${pct.toFixed(0)}%` : '—'}
                </span>
              </span>
            );
          })
        ) : (
          <span className="text-[11px] uppercase tracking-[0.08em]" style={{ color: tokens.textFaint }}>
            Phases start at charge
          </span>
        )}

        {phase.dtr !== undefined && (
          <span className="ml-auto text-[11px] font-semibold uppercase tracking-[0.08em] tabular-nums" style={{ color: tokens.forest }}>
            DTR {(phase.dtr * 100).toFixed(1)}%
          </span>
        )}
      </div>
    </div>
  );
}
