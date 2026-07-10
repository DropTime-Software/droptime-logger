/**
 * features/library — shared coffee/machine library loader.
 *
 * Tauri-only (the library lives in SQLite); in browser demo mode it returns
 * empty lists and no-op creators so the same UI renders as plain free-text
 * fields. Newly created rows are inserted into local state so a fresh "create"
 * shows up immediately without a full reload.
 */
import { useCallback, useEffect, useState } from 'react';
import { ipc, isTauri } from '../../bridge';
import type { ComboItem } from './Combobox';

function coffeeSublabel(origin?: string, process?: string): string | undefined {
  return [origin, process].filter(Boolean).join(' · ') || undefined;
}

function byName(a: ComboItem, b: ComboItem): number {
  return a.name.localeCompare(b.name, undefined, { sensitivity: 'base' });
}

export interface Library {
  coffees: ComboItem[];
  machines: ComboItem[];
  loading: boolean;
  tauri: boolean;
  createCoffee: (name: string) => Promise<ComboItem>;
  createMachine: (name: string, make?: string) => Promise<ComboItem>;
}

export function useLibrary(): Library {
  const tauri = isTauri();
  const [coffees, setCoffees] = useState<ComboItem[]>([]);
  const [machines, setMachines] = useState<ComboItem[]>([]);
  const [loading, setLoading] = useState(tauri);

  useEffect(() => {
    if (!tauri) return;
    let cancelled = false;
    Promise.all([ipc.listCoffees(), ipc.listMachines()])
      .then(([cs, ms]) => {
        if (cancelled) return;
        setCoffees(cs.map((c) => ({ id: c.id, name: c.name, sublabel: coffeeSublabel(c.origin, c.process) })));
        setMachines(ms.map((m) => ({ id: m.id, name: m.name, sublabel: m.make })));
      })
      .catch(() => undefined)
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [tauri]);

  const createCoffee = useCallback(async (name: string): Promise<ComboItem> => {
    const c = await ipc.saveCoffee({ name: name.trim() });
    const item: ComboItem = { id: c.id, name: c.name, sublabel: coffeeSublabel(c.origin, c.process) };
    setCoffees((prev) => [...prev.filter((p) => p.id !== item.id), item].sort(byName));
    return item;
  }, []);

  const createMachine = useCallback(async (name: string, make?: string): Promise<ComboItem> => {
    const m = await ipc.saveMachine({ name: name.trim(), make: make?.trim() || undefined });
    const item: ComboItem = { id: m.id, name: m.name, sublabel: m.make };
    setMachines((prev) => [...prev.filter((p) => p.id !== item.id), item].sort(byName));
    return item;
  }, []);

  return { coffees, machines, loading, tauri, createCoffee, createMachine };
}
