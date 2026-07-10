/**
 * Settings — the calm home for everything app-feel (owner: polisher).
 * Reached via the header gear (`actions.navigate('settings')`) and the native
 * menu's Settings… (⌘,) `menu://settings` event. Sections: Units, Sounds,
 * Auto-marking, Alerts, Updates, Setup, About, and the Droptime Cloud pane.
 */
import { useCallback, useEffect, useState } from 'react';
import { ArrowSquareOut } from '@phosphor-icons/react';
import type { AppMode, AppInfo } from '../bridge';
import { ipc } from '../bridge';
import { useSession } from '../state/SessionProvider';
import { useSettings } from '../settings/SettingsProvider';
import { Button } from '../components/ui';
import { Section, SegmentedToggle, SettingRow, Toggle } from '../features/appShell/controls';
import { useStringSetting, useToggleSetting } from '../features/appShell/settings';
import { openExternal } from '../features/appShell/open';
import { CLOUD_URL } from '../features/appShell/constants';
import { runUpdateCheck } from '../features/appShell/update';
import { setSoundCuesEnabled } from '../features/alerts/sound';
import { AlertsEditor } from '../features/alerts/AlertsEditor';

const BROWSER_INFO: AppInfo = { version: '0.1.0', os: 'browser', arch: '' };

type CheckState =
  | { kind: 'idle' }
  | { kind: 'checking' }
  | { kind: 'current'; version: string }
  | { kind: 'newer'; version: string; url: string }
  | { kind: 'error' };

export function SettingsScreen({ mode }: { mode: AppMode }) {
  const { state, actions, capturing } = useSession();
  const { unit, setUnit } = useSettings();

  const [soundOn, setSoundOn] = useToggleSetting(mode, 'sound.cues', true);
  const [autoCharge, setAutoCharge] = useToggleSetting(mode, 'autoMark.charge', true);
  const [autoDrop, setAutoDrop] = useToggleSetting(mode, 'autoMark.drop', true);
  const [lastCheckRaw] = useStringSetting(mode, 'updates.lastCheckMs', '');

  const [info, setInfo] = useState<AppInfo>(BROWSER_INFO);
  const [check, setCheck] = useState<CheckState>({ kind: 'idle' });

  // Keep the global sound gate honest the moment the toggle flips.
  useEffect(() => {
    setSoundCuesEnabled(soundOn);
  }, [soundOn]);

  useEffect(() => {
    if (mode !== 'tauri') return;
    let cancelled = false;
    ipc
      .getAppInfo()
      .then((i) => !cancelled && setInfo(i))
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [mode]);

  const onCheckNow = useCallback(async () => {
    setCheck({ kind: 'checking' });
    const res = await runUpdateCheck(mode);
    if (!res) {
      setCheck({ kind: 'error' });
    } else if (res.isNewer) {
      setCheck({ kind: 'newer', version: res.latestVersion, url: res.url });
    } else {
      setCheck({ kind: 'current', version: res.currentVersion });
    }
  }, [mode]);

  const back = state.finalSummary ? 'summary' : capturing ? 'live' : 'setup';

  return (
    <div className="h-full w-full overflow-y-auto">
      <div className="mx-auto flex min-h-full w-full max-w-[820px] flex-col gap-7 px-6 py-8 lg:px-10">
        <div className="flex flex-wrap items-end justify-between gap-4">
          <div>
            <h1 className="text-2xl font-bold tracking-[-0.06em] text-pine">Settings</h1>
            <p className="mt-1 text-sm text-ink/55">
              {mode === 'tauri'
                ? 'Everything the logger remembers, in one place.'
                : 'Browser demo — settings persist only to this browser.'}
            </p>
          </div>
          <Button onClick={() => actions.navigate(back)}>Back</Button>
        </div>

        {/* Units */}
        <Section
          title="Units"
          description="How temperatures are shown. Data is always captured in °F — this only changes the display."
          right={
            <SegmentedToggle
              ariaLabel="Temperature unit"
              value={unit}
              options={[
                { value: 'F', label: '°F' },
                { value: 'C', label: '°C' },
              ]}
              onChange={(u) => setUnit(u)}
            />
          }
        />

        {/* Sounds */}
        <Section title="Sounds" description="Audible cues for alerts and roast events.">
          <SettingRow
            label="Sound cues"
            hint="Play synthesized tones for alerts and projected first-crack."
            control={<Toggle checked={soundOn} onChange={setSoundOn} label="Sound cues" />}
          />
        </Section>

        {/* Auto-marking */}
        <Section
          title="Auto-marking"
          description="Let the logger detect CHARGE and DROP for you on a connected roaster. You can always undo an auto-mark."
        >
          <SettingRow
            label="Auto-detect charge"
            hint="Marks CHARGE the moment beans hit the drum."
            control={<Toggle checked={autoCharge} onChange={setAutoCharge} label="Auto-detect charge" />}
          />
          <SettingRow
            label="Auto-detect drop"
            hint="Marks DROP when the bean temperature falls off."
            control={<Toggle checked={autoDrop} onChange={setAutoDrop} label="Auto-detect drop" />}
          />
        </Section>

        {/* Alerts */}
        <Section
          title="Alerts"
          description="Get a heads-up — a sound, a spoken phrase, or a desktop notification — when a roast reaches a temperature or a time. Alerts never touch the roaster."
        >
          <AlertsEditor mode={mode} unit={unit} />
        </Section>

        {/* Updates */}
        <Section title="Updates" description="Droptime Logger is notice-only — it never installs anything on its own.">
          <SettingRow
            label="Current version"
            hint={lastCheckRaw ? `Last checked ${formatWhen(lastCheckRaw)}` : 'Not checked yet'}
            control={<span className="text-sm font-semibold tabular-nums text-pine">v{info.version}</span>}
          />
          <div className="flex flex-wrap items-center gap-3 pt-1">
            <Button
              variant="ghost"
              onClick={() => void onCheckNow()}
              disabled={check.kind === 'checking' || mode !== 'tauri'}
            >
              {check.kind === 'checking' ? 'Checking…' : 'Check now'}
            </Button>
            {mode !== 'tauri' ? (
              <span className="text-xs text-ink/45">Update checks run in the desktop app.</span>
            ) : check.kind === 'current' ? (
              <span className="text-sm text-grass">You're up to date.</span>
            ) : check.kind === 'newer' ? (
              <button
                type="button"
                onClick={() => void openExternal(mode, check.url)}
                className="dt-focus-ring inline-flex items-center gap-1 text-sm font-semibold text-forest hover:underline"
              >
                v{check.version} available <ArrowSquareOut size={14} weight="bold" />
              </button>
            ) : check.kind === 'error' ? (
              <span className="text-sm text-ink/50">Couldn't check right now.</span>
            ) : null}
          </div>
        </Section>

        {/* Setup */}
        <Section
          title="Setup"
          description="Re-run the first-time setup — pick your roaster, probe channels, and defaults."
          right={
            capturing ? (
              <span className="text-xs text-ink/45">Finish the roast in progress first</span>
            ) : (
              <Button variant="ghost" onClick={() => actions.navigate('wizard')}>
                Run setup wizard again
              </Button>
            )
          }
        />

        {/* About */}
        <Section title="About">
          <div className="rounded-xl bg-gray-50 p-4 text-sm">
            <div className="flex items-center justify-between py-1">
              <span className="text-ink/55">Version</span>
              <span className="font-medium tabular-nums text-pine">v{info.version}</span>
            </div>
            <div className="flex items-center justify-between py-1">
              <span className="text-ink/55">Platform</span>
              <span className="font-medium text-pine">
                {info.os}
                {info.arch ? ` · ${info.arch}` : ''}
              </span>
            </div>
            <div className="flex items-center justify-between py-1">
              <span className="text-ink/55">License</span>
              <span className="font-medium text-pine">Apache-2.0 / MIT — free and open source</span>
            </div>
          </div>
        </Section>

        {/* Droptime Cloud */}
        <CloudPane mode={mode} />
      </div>
    </div>
  );
}

function CloudPane({ mode }: { mode: AppMode }) {
  return (
    <section className="mt-1 overflow-hidden rounded-2xl bg-pine text-cream shadow-xl">
      <div className="relative p-6 sm:p-7">
        <div
          aria-hidden
          className="pointer-events-none absolute -right-10 -top-10 h-40 w-40 rounded-full"
          style={{ background: 'radial-gradient(circle, rgba(218,246,152,0.22), transparent 70%)' }}
        />
        <div className="relative">
          <div className="flex items-center gap-2">
            <span className="rounded-full bg-lemon/20 px-2.5 py-0.5 text-[10px] font-semibold uppercase tracking-[0.14em] text-lemon">
              Coming soon
            </span>
          </div>
          <h2 className="mt-3 text-xl font-bold tracking-[-0.04em] text-cream">
            Droptime Cloud<span className="text-lemon">.</span>
          </h2>
          <p className="mt-2 max-w-lg text-sm leading-relaxed text-cream/70">
            Sync your roast history, get AI roast readouts, and share with your team. Coming soon.
            The logger stays free and local-first, forever.
          </p>
          <div className="mt-4">
            <button
              type="button"
              onClick={() => void openExternal(mode, CLOUD_URL)}
              className="dt-focus-ring inline-flex items-center gap-1.5 rounded-lg bg-lemon px-4 py-2 text-sm font-semibold text-forest transition-colors hover:bg-cream"
            >
              Learn more <ArrowSquareOut size={15} weight="bold" />
            </button>
          </div>
        </div>
      </div>
    </section>
  );
}

function formatWhen(ms: string): string {
  const n = Number(ms);
  if (!Number.isFinite(n) || n <= 0) return 'never';
  try {
    return new Date(n).toLocaleString(undefined, {
      month: 'short',
      day: 'numeric',
      hour: 'numeric',
      minute: '2-digit',
    });
  } catch {
    return 'recently';
  }
}
