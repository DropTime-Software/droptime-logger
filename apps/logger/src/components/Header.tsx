import { Gear } from '@phosphor-icons/react';
import { useSettings } from '../settings/SettingsProvider';
import { useSession } from '../state/SessionProvider';
import type { ConnState } from '../state/types';
import { AppShell } from '../features/appShell/AppShell';

const DOT: Record<ConnState, { color: string; label: string }> = {
  idle: { color: 'rgba(253,247,238,0.3)', label: 'Idle' },
  connected: { color: '#daf698', label: 'Live' },
  reconnected: { color: '#daf698', label: 'Reconnected' },
  gap: { color: '#e8a44d', label: 'Signal gap' },
  flatline: { color: '#e8a44d', label: 'Flat signal' },
  disconnected: { color: '#fe6e51', label: 'Disconnected' },
  ended: { color: 'rgba(253,247,238,0.4)', label: 'Ended' },
};

export function Header({ mode }: { mode: 'tauri' | 'browser' }) {
  const { unit, toggleUnit } = useSettings();
  const { state, actions } = useSession();
  const dot = DOT[state.connection];
  const onSettings = state.screen === 'settings';

  // The header lives on the dark-pine chrome (web-app AppShell sidebar idiom):
  // cream text, lemon accents, brand badge in the pale-lemon circle.
  return (
    <>
    <header className="flex items-center justify-between px-5 py-3">
      <div className="flex items-center gap-3">
        <span className="flex h-8 w-8 items-center justify-center rounded-full bg-[#f5eeba] text-[15px] font-bold leading-none text-[#5a7a32]">
          D
        </span>
        <span className="text-[17px] font-semibold tracking-[-0.5px] text-cream">
          Droptime Logger<span className="text-lemon">.</span>
        </span>
        {mode === 'browser' ? (
          <span className="rounded-full border border-cream/20 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-cream/50">
            Browser demo
          </span>
        ) : null}
      </div>

      <div className="flex items-center gap-4">
        <div className="flex items-center gap-2" title={dot.label}>
          <span
            className="inline-block h-2.5 w-2.5 rounded-full"
            style={{ background: dot.color, boxShadow: `0 0 8px ${dot.color}` }}
          />
          <span className="text-xs text-cream/55">{dot.label}</span>
        </div>

        <button
          onClick={toggleUnit}
          className="flex items-center overflow-hidden rounded-full border border-cream/20 text-xs font-semibold focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-lemon"
          aria-label="Toggle temperature unit"
        >
          <span
            className={`px-2.5 py-1 transition ${
              unit === 'F' ? 'bg-lemon text-pine' : 'text-cream/55'
            }`}
          >
            °F
          </span>
          <span
            className={`px-2.5 py-1 transition ${
              unit === 'C' ? 'bg-lemon text-pine' : 'text-cream/55'
            }`}
          >
            °C
          </span>
        </button>

        <button
          onClick={() => actions.navigate('settings')}
          aria-label="Settings"
          title="Settings"
          className="flex h-8 w-8 items-center justify-center rounded-full border transition focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-lemon"
          style={{
            borderColor: onSettings ? 'transparent' : 'rgba(253,247,238,0.2)',
            background: onSettings ? 'var(--color-lemon)' : 'transparent',
            color: onSettings ? 'var(--color-pine)' : 'rgba(253,247,238,0.65)',
          }}
        >
          <Gear size={16} weight="bold" />
        </button>
      </div>
    </header>
    <AppShell mode={mode} />
    </>
  );
}
