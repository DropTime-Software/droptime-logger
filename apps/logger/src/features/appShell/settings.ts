/**
 * Generic keyed settings access for the app-feel features (CONTRACTS §8
 * settings registry). `SettingsProvider` owns the single `units` key; the many
 * other keys — `sound.cues`, `autoMark.*`, `alerts.rules`, `window.state`,
 * `updates.*` — are read/written here through the same generic backend
 * (`get_setting`/`set_setting` in Tauri, `localStorage` in the browser demo).
 *
 * Mode is passed explicitly (never re-detected) to match the app's threading
 * convention: screens/hooks already hold `mode` (prop) or `state.mode`.
 */
import { useCallback, useEffect, useState } from 'react';
import { ipc, type AppMode } from '../../bridge';

const LS_PREFIX = 'droptime.';

/** Read a raw string setting; any failure resolves to `null` (never throws). */
export async function readSetting(mode: AppMode, key: string): Promise<string | null> {
  if (mode === 'tauri') {
    try {
      return await ipc.getSetting({ key });
    } catch {
      return null;
    }
  }
  try {
    return localStorage.getItem(LS_PREFIX + key);
  } catch {
    return null;
  }
}

/** Write a raw string setting (fire-and-forget; swallows failures). */
export async function writeSetting(mode: AppMode, key: string, value: string): Promise<void> {
  if (mode === 'tauri') {
    await ipc.setSetting({ key, value }).catch(() => undefined);
  } else {
    try {
      localStorage.setItem(LS_PREFIX + key, value);
    } catch {
      /* ignore quota / disabled storage */
    }
  }
}

/**
 * Reactive string setting. Starts at `fallback`, hydrates from the backend once
 * mounted, and writes through on update. `loaded` flips true after the first
 * read so callers can avoid flashing the fallback in async UIs.
 */
export function useStringSetting(
  mode: AppMode,
  key: string,
  fallback: string,
): readonly [string, (next: string) => void, boolean] {
  const [value, setValue] = useState(fallback);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    let cancelled = false;
    void readSetting(mode, key).then((raw) => {
      if (cancelled) return;
      if (raw != null) setValue(raw);
      setLoaded(true);
    });
    return () => {
      cancelled = true;
    };
  }, [mode, key]);

  const update = useCallback(
    (next: string) => {
      setValue(next);
      void writeSetting(mode, key, next);
    },
    [mode, key],
  );

  return [value, update, loaded] as const;
}

/**
 * Reactive on/off setting stored as the strings `'on'` / `'off'` (the registry
 * convention for `sound.cues` and `autoMark.*`). `defaultOn` decides the value
 * before the stored one loads and when nothing is stored yet.
 */
export function useToggleSetting(
  mode: AppMode,
  key: string,
  defaultOn: boolean,
): readonly [boolean, (next: boolean) => void, boolean] {
  const [raw, setRaw, loaded] = useStringSetting(mode, key, defaultOn ? 'on' : 'off');
  const on = raw !== 'off';
  const setOn = useCallback((next: boolean) => setRaw(next ? 'on' : 'off'), [setRaw]);
  return [on, setOn, loaded] as const;
}
