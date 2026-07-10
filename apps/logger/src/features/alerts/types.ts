/**
 * Alert rules (CONTRACTS §8). Rules live in the `alerts.rules` setting as a JSON
 * array and are evaluated in the webview at the ≤1Hz derived cadence, fire-once
 * per rule per roast. READ-ONLY: actions are notify / sound / speak only — an
 * alert never actuates hardware.
 */
import type { RoastEventKind, RoastMarkersSec } from '@droptime/roast-console';
import { formatTemp, mmss } from '@droptime/roast-console';
import type { TempUnit } from '../../settings/SettingsProvider';

export type AlertSound = 'tick' | 'chime';

/** BT crossed a threshold (canonical °F), or a fixed time past a marker event. */
export type AlertCondition =
  | { type: 'btAtLeast'; btF: number }
  | { type: 'timeAfterEvent'; event: RoastEventKind; afterSec: number };

export interface AlertAction {
  notify?: boolean;
  sound?: AlertSound;
  speak?: string;
}

export interface AlertRule {
  id: string;
  enabled: boolean;
  when: AlertCondition;
  action: AlertAction;
}

/** Events a `timeAfterEvent` rule can key off (only those with a marker time). */
export const ALERT_EVENTS: ReadonlyArray<{ value: RoastEventKind; label: string }> = [
  { value: 'charge', label: 'Charge' },
  { value: 'turning_point', label: 'Turning point' },
  { value: 'dry_end', label: 'Dry end' },
  { value: 'fc_start', label: 'First crack' },
  { value: 'fc_end', label: 'First crack end' },
  { value: 'drop', label: 'Drop' },
];

/** Seconds-from-charge for a marker event, or undefined when it isn't marked. */
export function eventTimeSec(
  event: RoastEventKind,
  markers: RoastMarkersSec,
  charged: boolean,
): number | undefined {
  switch (event) {
    case 'charge':
      return charged ? 0 : undefined;
    case 'turning_point':
      return markers.turningPointSec;
    case 'dry_end':
      return markers.dryEndSec;
    case 'fc_start':
      return markers.fcStartSec;
    case 'fc_end':
      return markers.fcEndSec;
    case 'drop':
      return markers.dropSec;
    default:
      return undefined;
  }
}

export interface AlertContext {
  btF?: number;
  /** seconds-from-charge (from derived state) */
  elapsedT: number;
  charged: boolean;
  markers: RoastMarkersSec;
}

/** True when the rule's condition is currently satisfied. Pure. */
export function conditionMet(when: AlertCondition, ctx: AlertContext): boolean {
  if (when.type === 'btAtLeast') {
    return ctx.btF !== undefined && ctx.btF >= when.btF;
  }
  // timeAfterEvent
  const base = eventTimeSec(when.event, ctx.markers, ctx.charged);
  if (base === undefined) return false;
  return ctx.elapsedT >= base + when.afterSec;
}

/** Human sentence for a rule — used as the notification body and TTS default. */
export function describeRule(when: AlertCondition, unit: TempUnit): string {
  if (when.type === 'btAtLeast') {
    return `Bean temp reached ${formatTemp(when.btF, unit)}`;
  }
  const label =
    ALERT_EVENTS.find((e) => e.value === when.event)?.label ?? when.event.replace(/_/g, ' ');
  if (when.afterSec <= 0) return `${label} reached`;
  return `${mmss(when.afterSec)} after ${label.toLowerCase()}`;
}

const isSound = (v: unknown): v is AlertSound => v === 'tick' || v === 'chime';

/** Parse + validate the stored `alerts.rules` JSON. Bad input → []. */
export function parseRules(raw: string | null): AlertRule[] {
  if (!raw) return [];
  let data: unknown;
  try {
    data = JSON.parse(raw);
  } catch {
    return [];
  }
  if (!Array.isArray(data)) return [];
  const out: AlertRule[] = [];
  for (const item of data) {
    const rule = coerceRule(item);
    if (rule) out.push(rule);
  }
  return out;
}

function coerceRule(item: unknown): AlertRule | null {
  if (!item || typeof item !== 'object') return null;
  const r = item as Record<string, unknown>;
  const when = coerceCondition(r.when);
  if (!when) return null;
  const id = typeof r.id === 'string' && r.id ? r.id : cryptoId();
  const enabled = r.enabled !== false;
  const action = coerceAction(r.action);
  return { id, enabled, when, action };
}

function coerceCondition(w: unknown): AlertCondition | null {
  if (!w || typeof w !== 'object') return null;
  const c = w as Record<string, unknown>;
  if (c.type === 'btAtLeast' && typeof c.btF === 'number' && Number.isFinite(c.btF)) {
    return { type: 'btAtLeast', btF: c.btF };
  }
  if (
    c.type === 'timeAfterEvent' &&
    typeof c.event === 'string' &&
    typeof c.afterSec === 'number' &&
    Number.isFinite(c.afterSec)
  ) {
    return { type: 'timeAfterEvent', event: c.event as RoastEventKind, afterSec: Math.max(0, c.afterSec) };
  }
  return null;
}

function coerceAction(a: unknown): AlertAction {
  if (!a || typeof a !== 'object') return {};
  const o = a as Record<string, unknown>;
  const action: AlertAction = {};
  if (o.notify === true) action.notify = true;
  if (isSound(o.sound)) action.sound = o.sound;
  if (typeof o.speak === 'string' && o.speak.trim()) action.speak = o.speak;
  return action;
}

export function serializeRules(rules: AlertRule[]): string {
  return JSON.stringify(rules);
}

/** Stable id, falling back when crypto.randomUUID is unavailable. */
export function cryptoId(): string {
  try {
    if (typeof crypto !== 'undefined' && crypto.randomUUID) return crypto.randomUUID();
  } catch {
    /* ignore */
  }
  return `rule-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

/** A sensible starter rule (BT target — the most common cue). */
export function newRule(): AlertRule {
  return { id: cryptoId(), enabled: true, when: { type: 'btAtLeast', btF: 400 }, action: { sound: 'chime' } };
}
