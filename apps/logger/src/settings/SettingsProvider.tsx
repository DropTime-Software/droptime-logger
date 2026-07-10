import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react';
import { ipc, type AppMode } from '../bridge';

export type TempUnit = 'F' | 'C';

const UNIT_KEY = 'units';
const LS_KEY = 'droptime.units';

interface SettingsContextValue {
  unit: TempUnit;
  setUnit(unit: TempUnit): void;
  toggleUnit(): void;
}

const SettingsContext = createContext<SettingsContextValue | null>(null);

export function useSettings(): SettingsContextValue {
  const ctx = useContext(SettingsContext);
  if (!ctx) throw new Error('useSettings must be used within <SettingsProvider>');
  return ctx;
}

function isUnit(v: unknown): v is TempUnit {
  return v === 'F' || v === 'C';
}

export function SettingsProvider({ mode, children }: { mode: AppMode; children: ReactNode }) {
  const [unit, setUnitState] = useState<TempUnit>('F');

  // Hydrate persisted preference.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const raw =
          mode === 'tauri'
            ? await ipc.getSetting({ key: UNIT_KEY })
            : localStorage.getItem(LS_KEY);
        if (!cancelled && isUnit(raw)) setUnitState(raw);
      } catch {
        /* keep default */
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [mode]);

  const setUnit = useCallback(
    (next: TempUnit) => {
      setUnitState(next);
      if (mode === 'tauri') {
        void ipc.setSetting({ key: UNIT_KEY, value: next }).catch(() => undefined);
      } else {
        try {
          localStorage.setItem(LS_KEY, next);
        } catch {
          /* ignore */
        }
      }
    },
    [mode],
  );

  const toggleUnit = useCallback(() => setUnit(unit === 'F' ? 'C' : 'F'), [unit, setUnit]);

  const value = useMemo(() => ({ unit, setUnit, toggleUnit }), [unit, setUnit, toggleUnit]);

  return <SettingsContext.Provider value={value}>{children}</SettingsContext.Provider>;
}
