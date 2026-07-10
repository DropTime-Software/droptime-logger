/**
 * features/library — a small type-ahead combobox with inline "create" support.
 *
 * Resolves free text to a library id (coffees_local / machines_local). Typing
 * clears any resolved id (free text); picking an item sets it; the optional
 * `onCreate` row saves a brand-new library row and adopts its id. Styled to the
 * house `dt-input` / `dt-card` conventions — no new global CSS.
 */
import { useEffect, useMemo, useRef, useState } from 'react';

export interface ComboItem {
  id: number;
  name: string;
  sublabel?: string;
}

export interface ComboboxProps {
  value: string;
  /** resolved library id; undefined = free text */
  selectedId?: number;
  items: ComboItem[];
  placeholder?: string;
  loading?: boolean;
  disabled?: boolean;
  id?: string;
  /** create a new library row from typed text; returns it (with a real id) */
  onCreate?: (name: string) => Promise<ComboItem>;
  createLabel?: (name: string) => string;
  onChange: (value: string, selectedId: number | undefined) => void;
}

export function Combobox({
  value,
  selectedId,
  items,
  placeholder,
  loading,
  disabled,
  id,
  onCreate,
  createLabel,
  onChange,
}: ComboboxProps) {
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const [creating, setCreating] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

  const query = value.trim().toLowerCase();
  const filtered = useMemo(() => {
    if (!query) return items;
    return items.filter((it) => it.name.toLowerCase().includes(query));
  }, [items, query]);

  const exactMatch = useMemo(
    () => items.some((it) => it.name.trim().toLowerCase() === query),
    [items, query],
  );
  const canCreate = !!onCreate && query.length > 0 && !exactMatch;
  const rowCount = filtered.length + (canCreate ? 1 : 0);

  // Close on outside click.
  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', onDoc);
    return () => document.removeEventListener('mousedown', onDoc);
  }, [open]);

  async function pickCreate() {
    if (!onCreate || creating) return;
    const name = value.trim();
    if (!name) return;
    setCreating(true);
    try {
      const item = await onCreate(name);
      onChange(item.name, item.id);
      setOpen(false);
    } catch {
      // best-effort; leave the field as free text
    } finally {
      setCreating(false);
    }
  }

  function pick(item: ComboItem) {
    onChange(item.name, item.id);
    setOpen(false);
  }

  function onKeyDown(e: React.KeyboardEvent<HTMLInputElement>) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setOpen(true);
      setActive((a) => Math.min(a + 1, Math.max(0, rowCount - 1)));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setActive((a) => Math.max(a - 1, 0));
    } else if (e.key === 'Enter') {
      if (!open) return;
      e.preventDefault();
      if (active < filtered.length) {
        const item = filtered[active];
        if (item) pick(item);
      } else if (canCreate) {
        void pickCreate();
      }
    } else if (e.key === 'Escape') {
      setOpen(false);
    }
  }

  return (
    <div ref={rootRef} className="relative">
      <input
        id={id}
        className="dt-input dt-focus-ring"
        value={value}
        placeholder={placeholder}
        disabled={disabled}
        autoComplete="off"
        onChange={(e) => {
          onChange(e.target.value, undefined);
          setOpen(true);
          setActive(0);
        }}
        onFocus={() => setOpen(true)}
        onKeyDown={onKeyDown}
        aria-expanded={open}
        role="combobox"
        aria-controls={id ? `${id}-listbox` : undefined}
      />
      {selectedId != null ? (
        <span
          className="pointer-events-none absolute right-3 top-1/2 -translate-y-1/2 rounded-full bg-mint px-2 py-0.5 text-[10px] font-semibold text-forest"
          title="Saved in your library"
        >
          library
        </span>
      ) : null}

      {open && (filtered.length > 0 || canCreate || loading) ? (
        <div
          id={id ? `${id}-listbox` : undefined}
          role="listbox"
          className="dt-card absolute z-30 mt-1 max-h-64 w-full overflow-y-auto p-1 shadow-2xl"
        >
          {loading ? <p className="px-3 py-2 text-xs text-ink/50">Loading…</p> : null}
          {filtered.map((it, i) => (
            <button
              key={it.id}
              type="button"
              role="option"
              aria-selected={i === active}
              onMouseEnter={() => setActive(i)}
              onClick={() => pick(it)}
              className="dt-focus-ring flex w-full items-center justify-between gap-3 rounded-lg px-3 py-2 text-left text-sm"
              style={{ background: i === active ? 'rgba(218,246,152,0.35)' : 'transparent' }}
            >
              <span className="min-w-0 truncate font-medium text-pine">{it.name}</span>
              {it.sublabel ? (
                <span className="shrink-0 truncate text-xs text-ink/45">{it.sublabel}</span>
              ) : null}
            </button>
          ))}
          {canCreate ? (
            <button
              type="button"
              role="option"
              aria-selected={active >= filtered.length}
              onMouseEnter={() => setActive(filtered.length)}
              onClick={() => void pickCreate()}
              disabled={creating}
              className="dt-focus-ring flex w-full items-center gap-2 rounded-lg px-3 py-2 text-left text-sm text-forest"
              style={{
                background: active >= filtered.length ? 'rgba(218,246,152,0.35)' : 'transparent',
              }}
            >
              <span className="text-base leading-none">+</span>
              <span className="truncate">
                {creating ? 'Saving…' : (createLabel?.(value.trim()) ?? `Create “${value.trim()}”`)}
              </span>
            </button>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
