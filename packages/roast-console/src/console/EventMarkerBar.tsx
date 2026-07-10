'use client';

/**
 * EventMarkerBar — the bottom band of the console. The five canonical taps
 * (charge → dry end → first crack → FC end → drop) as huge touch targets
 * (≥ 56 px tall): the NEXT expected marker is the prominent lemon button,
 * later unmarked steps stay reachable but quiet, and marked steps collapse
 * to compact chips showing their m:ss with a per-event undo.
 */

import { ArrowLineDown, ArrowUUpLeft, Fire, Sparkle, SunDim, Waveform } from '@phosphor-icons/react';
import type { Icon } from '@phosphor-icons/react';
import type { RoastEventKind, RoastMarkersSec } from '../types';
import { tokens } from '../tokens';
import { mmss } from '../chart/format';

export interface EventMarkerBarProps {
  markers: RoastMarkersSec;
  charged: boolean;
  onMark: (kind: RoastEventKind) => void;
  onUndo: (kind: RoastEventKind) => void;
  /** pre-session / no capture running: everything inert */
  disabled?: boolean;
}

interface StepDef {
  kind: RoastEventKind;
  label: string;
  IconCmp: Icon;
  timeSec: number | undefined;
  marked: boolean;
}

export function EventMarkerBar({ markers, charged, onMark, onUndo, disabled = false }: EventMarkerBarProps) {
  const steps: StepDef[] = [
    { kind: 'charge', label: 'Charge', IconCmp: Fire, timeSec: charged ? 0 : undefined, marked: charged },
    { kind: 'dry_end', label: 'Dry end', IconCmp: SunDim, timeSec: markers.dryEndSec, marked: markers.dryEndSec !== undefined },
    { kind: 'fc_start', label: 'First crack', IconCmp: Sparkle, timeSec: markers.fcStartSec, marked: markers.fcStartSec !== undefined },
    { kind: 'fc_end', label: 'FC end', IconCmp: Waveform, timeSec: markers.fcEndSec, marked: markers.fcEndSec !== undefined },
    { kind: 'drop', label: 'Drop', IconCmp: ArrowLineDown, timeSec: markers.dropSec, marked: markers.dropSec !== undefined },
  ];
  const nextIdx = steps.findIndex((s) => !s.marked);

  return (
    <div className="flex w-full items-stretch gap-2">
      {steps.map((s, i) => {
        if (s.marked) {
          return (
            <MarkedChip
              key={s.kind}
              step={s}
              disabled={disabled}
              onUndo={() => onUndo(s.kind)}
            />
          );
        }

        const isNext = i === nextIdx;
        // out-of-order marking is allowed once charged (roasters skip FC end);
        // nothing but CHARGE is tappable before charge
        const canMark = !disabled && (s.kind === 'charge' ? !charged : charged);

        if (isNext) {
          return (
            <button
              key={s.kind}
              type="button"
              disabled={!canMark}
              onClick={() => onMark(s.kind)}
              aria-label={`Mark ${s.label}`}
              className="flex min-h-14 min-w-0 flex-[2] items-center justify-center gap-2.5 rounded-xl text-base font-bold uppercase tracking-[0.06em] shadow-[0px_0px_4px_0px_rgba(0,0,0,0.15)] transition-transform active:scale-[0.99] disabled:cursor-not-allowed disabled:opacity-35 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#024522]"
              style={{ background: tokens.lemon, color: tokens.pine }}
            >
              <s.IconCmp size={20} weight="fill" />
              <span className="truncate">{s.label}</span>
            </button>
          );
        }

        return (
          <button
            key={s.kind}
            type="button"
            disabled={!canMark}
            onClick={() => onMark(s.kind)}
            aria-label={`Mark ${s.label}`}
            className="flex min-h-14 min-w-0 flex-1 items-center justify-center gap-2 rounded-xl border text-sm font-semibold uppercase tracking-[0.06em] transition-colors hover:bg-[rgba(13,36,24,0.04)] disabled:cursor-not-allowed disabled:opacity-35 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#024522]"
            style={{ borderColor: tokens.hairline, color: tokens.textDim, background: 'transparent' }}
          >
            <s.IconCmp size={16} />
            <span className="truncate">{s.label}</span>
          </button>
        );
      })}
    </div>
  );
}

function MarkedChip({
  step,
  disabled,
  onUndo,
}: {
  step: StepDef;
  disabled: boolean;
  onUndo: () => void;
}) {
  return (
    <div
      className="flex min-h-14 flex-none items-center gap-2.5 rounded-xl border py-1.5 pl-3 pr-1.5"
      style={{ borderColor: tokens.hairline }}
    >
      <step.IconCmp size={16} style={{ color: tokens.textDim }} />
      <div className="leading-tight">
        <div
          className="text-[9px] font-semibold uppercase tracking-[0.1em] whitespace-nowrap"
          style={{ color: tokens.textDim }}
        >
          {step.label}
        </div>
        <div className="text-lg font-semibold leading-none tabular-nums" style={{ color: tokens.text }}>
          {step.timeSec !== undefined ? mmss(step.timeSec) : '—'}
        </div>
      </div>
      <button
        type="button"
        disabled={disabled}
        onClick={onUndo}
        aria-label={`Undo ${step.label}`}
        title={`Undo ${step.label}`}
        className="flex h-9 w-9 items-center justify-center rounded-lg transition-colors hover:bg-[rgba(13,36,24,0.05)] disabled:cursor-not-allowed disabled:opacity-35 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#024522]"
        style={{ color: tokens.textDim }}
      >
        <ArrowUUpLeft size={15} />
      </button>
    </div>
  );
}
