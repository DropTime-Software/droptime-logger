/**
 * Step 1 — Welcome. Sets the tone, lets the operator pick a path (connect a
 * real roaster vs. explore with bundled sample roasts), and captures the
 * display-unit preference up front. In browser demo mode there is no serial
 * hardware, so the connect path is shown as desktop-only and demo is the lane.
 */
import { PlugsConnected, CoffeeBean, CaretRight } from '@phosphor-icons/react';
import type { AppMode } from '../../../bridge';
import { SegmentedToggle } from '../../../features/appShell/controls';
import type { WizardPath } from '../machine';

export function WelcomeStep({
  mode,
  path,
  onPathChange,
  unit,
  onUnitChange,
}: {
  mode: AppMode;
  path: WizardPath;
  onPathChange: (path: WizardPath) => void;
  unit: 'F' | 'C';
  onUnitChange: (unit: 'F' | 'C') => void;
}) {
  const deviceAvailable = mode === 'tauri';

  return (
    <div className="flex flex-col">
      <div className="mb-7">
        <p className="text-sm leading-relaxed text-ink/60">
          Free, local-first roast logging — your beans, your machine, your data. This takes
          about a minute, and you can change everything later in Settings.
        </p>
      </div>

      <div className="mb-2 text-[11px] font-semibold uppercase tracking-[0.14em] text-forest">
        How do you want to start?
      </div>
      <div className="grid gap-3 sm:grid-cols-2">
        <PathCard
          active={path === 'device'}
          disabled={!deviceAvailable}
          icon={<PlugsConnected size={22} weight="duotone" />}
          title="Connect my roaster"
          body="Log live from a TC4 or Arduino thermocouple board over USB. Read-only — the logger never drives your roaster."
          badge={deviceAvailable ? undefined : 'Desktop app'}
          onSelect={() => deviceAvailable && onPathChange('device')}
        />
        <PathCard
          active={path === 'demo'}
          icon={<CoffeeBean size={22} weight="duotone" />}
          title="Explore with sample roasts"
          body="Replay bundled roast profiles to learn the live console first. Nothing is saved — you can wire up a roaster anytime."
          onSelect={() => onPathChange('demo')}
        />
      </div>

      <div className="mt-7 flex flex-wrap items-center justify-between gap-4 border-t border-line pt-6">
        <div>
          <div className="text-sm font-semibold text-pine">Temperature units</div>
          <div className="mt-0.5 text-xs leading-relaxed text-ink/50">
            Data is always captured in °F — this only changes what you see.
          </div>
        </div>
        <SegmentedToggle
          ariaLabel="Temperature unit"
          value={unit}
          options={[
            { value: 'F', label: '°F' },
            { value: 'C', label: '°C' },
          ]}
          onChange={onUnitChange}
        />
      </div>
    </div>
  );
}

function PathCard({
  active,
  disabled,
  icon,
  title,
  body,
  badge,
  onSelect,
}: {
  active: boolean;
  disabled?: boolean;
  icon: React.ReactNode;
  title: string;
  body: string;
  badge?: string;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onSelect}
      disabled={disabled}
      aria-pressed={active}
      className="dt-focus-ring group relative flex flex-col rounded-2xl border p-5 text-left transition disabled:cursor-not-allowed"
      style={{
        borderColor: active ? 'var(--color-forest)' : 'var(--color-line)',
        background: active ? 'rgba(218,246,152,0.35)' : '#ffffff',
        boxShadow: '0px 0px 4px 0px rgba(0,0,0,0.08)',
        opacity: disabled ? 0.62 : 1,
      }}
    >
      <div className="flex items-center justify-between">
        <span
          className="flex h-11 w-11 items-center justify-center rounded-xl"
          style={{
            background: active ? 'var(--color-forest)' : 'var(--color-mint)',
            color: active ? 'var(--color-lemon)' : 'var(--color-forest)',
          }}
        >
          {icon}
        </span>
        {badge ? (
          <span className="rounded-full bg-mint px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-forest">
            {badge}
          </span>
        ) : (
          <CaretRight
            size={16}
            weight="bold"
            className="text-ink/25 transition-transform group-hover:translate-x-0.5"
            style={{ color: active ? 'var(--color-forest)' : undefined }}
          />
        )}
      </div>
      <div className="mt-4 font-semibold tracking-[-0.02em] text-pine">{title}</div>
      <div className="mt-1 text-xs leading-relaxed text-ink/55">{body}</div>
    </button>
  );
}
