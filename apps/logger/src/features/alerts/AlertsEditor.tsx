/**
 * AlertsEditor — the friendly rule builder for `alerts.rules` (CONTRACTS §8).
 * List / add / edit / delete / toggle rules. Thresholds are entered in the
 * user's DISPLAY unit but stored canonical °F; times are entered mm:ss but
 * stored as seconds. READ-ONLY at heart — the actions only notify / sound /
 * speak. Used from SettingsScreen.
 */
import { useEffect, useState, type ReactNode } from 'react';
import { Bell, Play, Plus, SpeakerHigh, Trash } from '@phosphor-icons/react';
import { fToC, type RoastEventKind } from '@droptime/roast-console';
import type { AppMode } from '../../bridge';
import type { TempUnit } from '../../settings/SettingsProvider';
import { Button, TextInput } from '../../components/ui';
import { SegmentedToggle, Toggle } from '../appShell/controls';
import { readSetting, writeSetting } from '../appShell/settings';
import { playCue } from './sound';
import {
  ALERT_EVENTS,
  describeRule,
  newRule,
  parseRules,
  serializeRules,
  type AlertAction,
  type AlertRule,
  type AlertSound,
} from './types';

const cToF = (c: number): number => (c * 9) / 5 + 32;

export function AlertsEditor({ mode, unit }: { mode: AppMode; unit: TempUnit }) {
  const [rules, setRules] = useState<AlertRule[]>([]);

  useEffect(() => {
    let cancelled = false;
    void readSetting(mode, 'alerts.rules').then((raw) => {
      if (!cancelled) setRules(parseRules(raw));
    });
    return () => {
      cancelled = true;
    };
  }, [mode]);

  function persist(next: AlertRule[]) {
    setRules(next);
    void writeSetting(mode, 'alerts.rules', serializeRules(next));
  }

  const patch = (id: string, fn: (r: AlertRule) => AlertRule) =>
    persist(rules.map((r) => (r.id === id ? fn(r) : r)));

  return (
    <div className="flex flex-col gap-3">
      {rules.length === 0 ? (
        <p className="rounded-xl bg-gray-50 px-4 py-6 text-center text-sm text-ink/50">
          No alerts yet. Add one to get a heads-up — a sound, a spoken cue, or a desktop
          notification — the moment a roast hits a temperature or time.
        </p>
      ) : (
        rules.map((rule) => (
          <RuleCard
            key={rule.id}
            rule={rule}
            unit={unit}
            onToggle={(on) => patch(rule.id, (r) => ({ ...r, enabled: on }))}
            onChange={(next) => patch(rule.id, () => next)}
            onDelete={() => persist(rules.filter((r) => r.id !== rule.id))}
          />
        ))
      )}

      <div>
        <Button variant="ghost" onClick={() => persist([...rules, newRule()])}>
          <Plus size={16} weight="bold" />
          Add alert
        </Button>
      </div>
    </div>
  );
}

function RuleCard({
  rule,
  unit,
  onToggle,
  onChange,
  onDelete,
}: {
  rule: AlertRule;
  unit: TempUnit;
  onToggle: (on: boolean) => void;
  onChange: (next: AlertRule) => void;
  onDelete: () => void;
}) {
  const setWhen = (when: AlertRule['when']) => onChange({ ...rule, when });
  const setAction = (action: AlertAction) => onChange({ ...rule, action });

  const displayBt =
    rule.when.type === 'btAtLeast'
      ? Math.round(unit === 'C' ? fToC(rule.when.btF) : rule.when.btF)
      : 0;

  const afterSec = rule.when.type === 'timeAfterEvent' ? rule.when.afterSec : 0;
  const mins = Math.floor(afterSec / 60);
  const secs = Math.round(afterSec % 60);

  return (
    <div
      className="rounded-2xl border border-line p-4"
      style={{ opacity: rule.enabled ? 1 : 0.6 }}
    >
      {/* header: enable + delete */}
      <div className="mb-3 flex items-center justify-between">
        <Toggle checked={rule.enabled} onChange={onToggle} label="Enable alert" />
        <button
          type="button"
          onClick={onDelete}
          aria-label="Delete alert"
          className="dt-focus-ring rounded-md p-1.5 text-ink/35 transition-colors hover:text-red-600"
        >
          <Trash size={16} weight="bold" />
        </button>
      </div>

      {/* WHEN */}
      <div className="flex flex-wrap items-center gap-2 text-sm text-ink/70">
        <span className="font-semibold text-forest">When</span>
        <SegmentedToggle
          ariaLabel="Condition type"
          value={rule.when.type}
          options={[
            { value: 'btAtLeast', label: 'Bean temp' },
            { value: 'timeAfterEvent', label: 'Time after' },
          ]}
          onChange={(t) =>
            setWhen(
              t === 'btAtLeast'
                ? { type: 'btAtLeast', btF: unit === 'C' ? cToF(200) : 400 }
                : { type: 'timeAfterEvent', event: 'fc_start', afterSec: 60 },
            )
          }
        />

        {rule.when.type === 'btAtLeast' ? (
          <span className="inline-flex items-center gap-1.5">
            <span>reaches</span>
            <input
              type="number"
              inputMode="numeric"
              value={displayBt}
              onChange={(e) => {
                const v = Number(e.target.value);
                if (!Number.isFinite(v)) return;
                setWhen({ type: 'btAtLeast', btF: unit === 'C' ? cToF(v) : v });
              }}
              className="dt-input dt-focus-ring !h-9 w-20 !px-2 text-center"
            />
            <span className="text-ink/55">°{unit}</span>
          </span>
        ) : (
          <span className="inline-flex flex-wrap items-center gap-1.5">
            <input
              type="number"
              inputMode="numeric"
              min={0}
              value={mins}
              onChange={(e) => {
                const m = Math.max(0, Math.floor(Number(e.target.value) || 0));
                setWhen({ type: 'timeAfterEvent', event: rule.when.type === 'timeAfterEvent' ? rule.when.event : 'fc_start', afterSec: m * 60 + secs });
              }}
              className="dt-input dt-focus-ring !h-9 w-14 !px-2 text-center"
            />
            <span className="text-ink/55">min</span>
            <input
              type="number"
              inputMode="numeric"
              min={0}
              max={59}
              value={secs}
              onChange={(e) => {
                const s = Math.min(59, Math.max(0, Math.floor(Number(e.target.value) || 0)));
                setWhen({ type: 'timeAfterEvent', event: rule.when.type === 'timeAfterEvent' ? rule.when.event : 'fc_start', afterSec: mins * 60 + s });
              }}
              className="dt-input dt-focus-ring !h-9 w-14 !px-2 text-center"
            />
            <span className="text-ink/55">sec after</span>
            <select
              value={rule.when.type === 'timeAfterEvent' ? rule.when.event : 'fc_start'}
              onChange={(e) =>
                setWhen({
                  type: 'timeAfterEvent',
                  event: e.target.value as RoastEventKind,
                  afterSec,
                })
              }
              className="dt-input dt-focus-ring !h-9 w-auto !px-2"
            >
              {ALERT_EVENTS.map((ev) => (
                <option key={ev.value} value={ev.value}>
                  {ev.label}
                </option>
              ))}
            </select>
          </span>
        )}
      </div>

      {/* THEN */}
      <div className="mt-3 flex flex-col gap-2 border-t border-line pt-3">
        <div className="flex flex-wrap items-center gap-x-5 gap-y-2 text-sm">
          <span className="font-semibold text-forest">Then</span>

          <ActionCheck
            icon={<Bell size={15} weight="bold" />}
            label="Notify"
            checked={!!rule.action.notify}
            onChange={(on) => setAction({ ...rule.action, notify: on || undefined })}
          />

          <ActionCheck
            icon={<SpeakerHigh size={15} weight="bold" />}
            label="Sound"
            checked={!!rule.action.sound}
            onChange={(on) => setAction({ ...rule.action, sound: on ? rule.action.sound ?? 'chime' : undefined })}
          />
          {rule.action.sound ? (
            <span className="inline-flex items-center gap-2">
              <SegmentedToggle
                ariaLabel="Sound"
                value={rule.action.sound}
                options={[
                  { value: 'tick', label: 'Tick' },
                  { value: 'chime', label: 'Chime' },
                ]}
                onChange={(s: AlertSound) => setAction({ ...rule.action, sound: s })}
              />
              <button
                type="button"
                aria-label="Preview sound"
                onClick={() => rule.action.sound && playCue(rule.action.sound)}
                className="dt-focus-ring rounded-md p-1.5 text-forest/70 transition-colors hover:text-forest"
              >
                <Play size={15} weight="fill" />
              </button>
            </span>
          ) : null}

          <ActionCheck
            label="Speak"
            checked={rule.action.speak !== undefined}
            onChange={(on) =>
              setAction({
                ...rule.action,
                speak: on ? rule.action.speak ?? describeRule(rule.when, unit) : undefined,
              })
            }
          />
        </div>

        {rule.action.speak !== undefined ? (
          <TextInput
            value={rule.action.speak}
            placeholder="Phrase to speak…"
            onChange={(e) => setAction({ ...rule.action, speak: e.target.value })}
            className="!h-10 max-w-md text-sm"
          />
        ) : null}
      </div>
    </div>
  );
}

function ActionCheck({
  label,
  checked,
  onChange,
  icon,
}: {
  label: string;
  checked: boolean;
  onChange: (on: boolean) => void;
  icon?: React.ReactNode;
}) {
  return (
    <label className="inline-flex cursor-pointer items-center gap-1.5 text-ink/70">
      <input
        type="checkbox"
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
        className="h-4 w-4 accent-[#024522]"
      />
      {icon}
      {label}
    </label>
  );
}
