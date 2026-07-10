/**
 * Keyboard-shortcuts overlay — opened by `?` and the native menu
 * (`menu://shortcuts`). Mirrors the live-screen bindings (see LiveScreen's
 * keydown handler and `state/roast.ts` KEY_TO_KIND) plus navigation keys.
 * Styled like RecoveryDialog.
 */
import { useEffect } from 'react';
import { Button } from '../../components/ui';

interface Shortcut {
  keys: string[];
  label: string;
}

const MARKING: Shortcut[] = [
  { keys: ['Space'], label: 'Mark next event' },
  { keys: ['C'], label: 'Charge' },
  { keys: ['D'], label: 'Dry end' },
  { keys: ['F'], label: 'First crack' },
  { keys: ['G'], label: 'First crack end' },
  { keys: ['X'], label: 'Drop' },
  { keys: ['U'], label: 'Undo last mark' },
];

const GENERAL: Shortcut[] = [
  { keys: ['⌘', ','], label: 'Settings' },
  { keys: ['?'], label: 'This shortcuts list' },
  { keys: ['Esc'], label: 'Close dialogs' },
];

export function ShortcutsOverlay({ onClose }: { onClose: () => void }) {
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key === 'Escape') onClose();
    }
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/30 px-6"
      onClick={onClose}
    >
      <div
        className="w-full max-w-lg rounded-3xl bg-white p-6 shadow-2xl"
        style={{ animation: 'dt-fade-in 0.2s ease' }}
        role="dialog"
        aria-modal="true"
        aria-labelledby="shortcuts-title"
        onClick={(e) => e.stopPropagation()}
      >
        <h2 id="shortcuts-title" className="text-lg font-bold tracking-[-0.04em] text-pine">
          Keyboard shortcuts
        </h2>
        <p className="mt-1 text-sm text-ink/55">
          Marking keys work on the live screen — no need to reach for the mouse mid-roast.
        </p>

        <div className="mt-5 grid gap-x-10 gap-y-6 sm:grid-cols-2">
          <Group title="Marking" items={MARKING} />
          <Group title="General" items={GENERAL} />
        </div>

        <div className="mt-6 flex justify-end">
          <Button variant="primary" onClick={onClose}>
            Done
          </Button>
        </div>
      </div>
    </div>
  );
}

function Group({ title, items }: { title: string; items: Shortcut[] }) {
  return (
    <div>
      <div className="mb-2 text-[11px] font-semibold uppercase tracking-[0.12em] text-forest">
        {title}
      </div>
      <dl className="space-y-1">
        {items.map((s) => (
          <div key={s.label} className="flex items-center justify-between gap-4 py-1">
            <dt className="text-sm text-ink/70">{s.label}</dt>
            <dd className="flex items-center gap-1">
              {s.keys.map((k) => (
                <kbd
                  key={k}
                  className="inline-flex min-w-[1.6rem] items-center justify-center rounded-md border border-line2 bg-gray-50 px-1.5 py-0.5 text-xs font-semibold tabular-nums text-pine"
                >
                  {k}
                </kbd>
              ))}
            </dd>
          </div>
        ))}
      </dl>
    </div>
  );
}
