/**
 * features/alerts — the alert rules engine (CONTRACTS §8, owner: polisher).
 *
 * Mounted once from LiveScreen as `useAlerts()`. Reads `alerts.rules` (settings
 * JSON) and evaluates them against the ≤1Hz derived tick from `useSession()`,
 * fire-once per rule per roast (reset when a new roast starts). READ-ONLY —
 * actions are OS notification / synthesized sound / spoken phrase only, never
 * hardware.
 */
import { useEffect, useRef } from 'react';
import { useSession } from '../../state/SessionProvider';
import { useSettings } from '../../settings/SettingsProvider';
import { readSetting } from '../appShell/settings';
import { notify } from './notify';
import { playCue, setSoundCuesEnabled } from './sound';
import { speak } from './speech';
import {
  conditionMet,
  describeRule,
  parseRules,
  type AlertRule,
} from './types';
import type { AppMode } from '../../bridge';
import type { TempUnit } from '../../settings/SettingsProvider';

function fireRule(rule: AlertRule, mode: AppMode, unit: TempUnit): void {
  const { action } = rule;
  const summary = describeRule(rule.when, unit);
  if (action.notify) void notify(mode, 'Droptime alert', summary);
  if (action.sound) playCue(action.sound);
  if (action.speak) speak(action.speak);
}

export function useAlerts(): void {
  const { state, derived } = useSession();
  const { unit } = useSettings();
  const mode = state.mode;
  const roastUuid = state.roastUuid;

  const rulesRef = useRef<AlertRule[]>([]);
  const firedRef = useRef<Set<string>>(new Set());
  const unitRef = useRef<TempUnit>(unit);
  unitRef.current = unit;

  // (Re)load rules + the global sound-cue gate whenever a roast begins.
  useEffect(() => {
    let cancelled = false;
    void readSetting(mode, 'alerts.rules').then((raw) => {
      if (!cancelled) rulesRef.current = parseRules(raw);
    });
    void readSetting(mode, 'sound.cues').then((raw) => {
      if (!cancelled) setSoundCuesEnabled(raw !== 'off');
    });
    return () => {
      cancelled = true;
    };
  }, [mode, roastUuid]);

  // Fresh roast → clear the fire-once ledger.
  useEffect(() => {
    firedRef.current = new Set();
  }, [roastUuid]);

  // Evaluate at the derived cadence (≤1Hz). `derived` is a fresh object each
  // recompute, so this effect re-runs exactly when there is new data.
  useEffect(() => {
    if (!roastUuid || state.finalSummary) return;
    const rules = rulesRef.current;
    if (rules.length === 0) return;

    const ctx = {
      btF: derived.latestSample?.btF,
      elapsedT: derived.elapsedT,
      charged: derived.charged,
      markers: state.markers,
    };

    for (const rule of rules) {
      if (!rule.enabled || firedRef.current.has(rule.id)) continue;
      if (conditionMet(rule.when, ctx)) {
        firedRef.current.add(rule.id);
        fireRule(rule, mode, unitRef.current);
      }
    }
  }, [derived, roastUuid, state.finalSummary, state.markers, mode]);
}
