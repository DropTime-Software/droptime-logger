/**
 * Small settings-surface primitives, styled to match `components/ui.tsx` and
 * the app's light card idiom (forest/lemon accents, hairline separations,
 * hierarchy from scale not boxes). Shared by SettingsScreen and AlertsEditor.
 */
import type { ReactNode } from 'react';

/** A titled settings section on the white card, separated by a hairline rule. */
export function Section({
  title,
  description,
  children,
  right,
}: {
  title: string;
  description?: string;
  children?: ReactNode;
  right?: ReactNode;
}) {
  return (
    <section className="border-t border-line pt-6">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h2 className="text-sm font-semibold uppercase tracking-[0.12em] text-forest">{title}</h2>
          {description ? (
            <p className="mt-1 max-w-xl text-sm leading-relaxed text-ink/55">{description}</p>
          ) : null}
        </div>
        {right ? <div className="shrink-0">{right}</div> : null}
      </div>
      {children ? <div className="mt-4">{children}</div> : null}
    </section>
  );
}

/** One label/description row with a control on the right (toggle, button, …). */
export function SettingRow({
  label,
  hint,
  control,
}: {
  label: string;
  hint?: string;
  control: ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-6 py-2.5">
      <div className="min-w-0">
        <div className="text-sm font-medium text-pine">{label}</div>
        {hint ? <div className="mt-0.5 text-xs leading-relaxed text-ink/50">{hint}</div> : null}
      </div>
      <div className="shrink-0">{control}</div>
    </div>
  );
}

/** iOS-style switch — lemon-on-forest when on, quiet gray when off. */
export function Toggle({
  checked,
  onChange,
  label,
  disabled,
}: {
  checked: boolean;
  onChange: (next: boolean) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className="dt-focus-ring relative inline-flex h-6 w-11 shrink-0 items-center rounded-full transition-colors disabled:opacity-50"
      style={{ background: checked ? 'var(--color-forest)' : 'rgba(36,36,36,0.18)' }}
    >
      <span
        className="inline-block h-5 w-5 rounded-full bg-white shadow transition-transform"
        style={{ transform: checked ? 'translateX(22px)' : 'translateX(2px)' }}
      />
    </button>
  );
}

/** Segmented two-choice pill (e.g. °F / °C), lemon-fill for the active side. */
export function SegmentedToggle<T extends string>({
  value,
  options,
  onChange,
  ariaLabel,
}: {
  value: T;
  options: ReadonlyArray<{ value: T; label: string }>;
  onChange: (next: T) => void;
  ariaLabel: string;
}) {
  return (
    <div
      role="group"
      aria-label={ariaLabel}
      className="inline-flex overflow-hidden rounded-full border border-line2 text-xs font-semibold"
    >
      {options.map((opt) => {
        const active = opt.value === value;
        return (
          <button
            key={opt.value}
            type="button"
            aria-pressed={active}
            onClick={() => onChange(opt.value)}
            className="dt-focus-ring px-3.5 py-1.5 transition-colors"
            style={{
              background: active ? 'var(--color-lemon)' : 'transparent',
              color: active ? 'var(--color-forest)' : 'rgba(36,36,36,0.55)',
            }}
          >
            {opt.label}
          </button>
        );
      })}
    </div>
  );
}
