/**
 * Setup wizard (owner: wizard — this directory is the mount point). First-run
 * gate + re-runnable from Settings/menu; reached via `actions.navigate('wizard')`.
 *
 * Flow (machine.ts owns the pure step sequence + SourcePin assembly):
 *   welcome → [connect → save]? → import → done
 * The connect/save steps are desktop-only and only appear on the "connect a
 * roaster" path; browser demo and the "explore" path go straight to import.
 *
 * Persistence: the display unit rides the shared SettingsProvider (writes the
 * `units` key); finishing or skipping writes `wizard.completed` so the first-run
 * gate doesn't fire again. Serial/preview device calls live in ConnectStep and
 * degrade gracefully while the real driver is stubbed.
 */
import { useMemo, useState } from 'react';
import { CaretLeft, CaretRight, Rocket } from '@phosphor-icons/react';
import { ipc, asLoggerError, type AppMode } from '../../bridge';
import { useSession } from '../../state/SessionProvider';
import { useSettings } from '../../settings/SettingsProvider';
import { Button } from '../../components/ui';
import {
  DEFAULT_DEVICE_CONFIG,
  STEP_META,
  clampIndex,
  hasPort,
  sourcePinJson,
  stepSequence,
  type DeviceConfig,
  type WizardPath,
} from './machine';
import { Stepper } from './Stepper';
import { WelcomeStep } from './steps/WelcomeStep';
import { ConnectStep } from './steps/ConnectStep';
import { NameStep } from './steps/NameStep';
import { ImportStep } from './steps/ImportStep';
import { DoneStep } from './steps/DoneStep';

const WIZARD_DONE_KEY = 'wizard.completed';
const WIZARD_DONE_LS = 'droptime.wizard.completed';

/** Mirror SettingsProvider's storage split: SQLite in Tauri, localStorage in the browser demo. */
async function persistWizardCompleted(mode: AppMode): Promise<void> {
  if (mode === 'tauri') {
    try {
      await ipc.setSetting({ key: WIZARD_DONE_KEY, value: 'true' });
    } catch {
      /* best-effort; the gate can still be re-run from Settings */
    }
  } else {
    try {
      localStorage.setItem(WIZARD_DONE_LS, 'true');
    } catch {
      /* ignore */
    }
  }
}

export function WizardScreen({ mode }: { mode: AppMode }) {
  const { actions, capturing, state } = useSession();
  const { unit, setUnit } = useSettings();

  const [path, setPath] = useState<WizardPath>('demo');
  const [device, setDevice] = useState<DeviceConfig>(DEFAULT_DEVICE_CONFIG);
  const [machineName, setMachineName] = useState('');
  const [machineMake, setMachineMake] = useState('');
  const [savedMachineName, setSavedMachineName] = useState<string | undefined>();
  const [importedCount, setImportedCount] = useState(0);

  const [stepIndex, setStepIndex] = useState(0);
  const [saving, setSaving] = useState(false);
  const [nameError, setNameError] = useState<string | null>(null);
  const [finishing, setFinishing] = useState(false);

  const sequence = useMemo(() => stepSequence(mode, path), [mode, path]);
  const index = clampIndex(stepIndex, sequence.length);
  const current = sequence[index] ?? 'welcome';

  const patchDevice = (patch: Partial<DeviceConfig>) => setDevice((d) => ({ ...d, ...patch }));

  function advance() {
    setStepIndex(clampIndex(index + 1, sequence.length));
  }

  function goBack() {
    setNameError(null);
    setStepIndex(clampIndex(index - 1, sequence.length));
  }

  async function saveThenAdvance() {
    const name = machineName.trim();
    if (!name) {
      setNameError('Give your roaster a name to save it.');
      return;
    }
    setSaving(true);
    setNameError(null);
    try {
      const saved = await ipc.saveMachine({
        name,
        make: machineMake.trim() || undefined,
        sourcePinJson: hasPort(device) ? sourcePinJson(device) : undefined,
      });
      setSavedMachineName(saved.name);
      advance();
    } catch (err) {
      setNameError(asLoggerError(err).message || 'Could not save the roaster. Try again.');
    } finally {
      setSaving(false);
    }
  }

  async function finishWizard() {
    setFinishing(true);
    await persistWizardCompleted(mode);
    // Never strand a live session: return to it (or its summary) if one is
    // recording; only a fresh install lands on the setup screen.
    actions.navigate(state.finalSummary ? 'summary' : capturing ? 'live' : 'setup');
  }

  function onPrimary() {
    if (current === 'done') {
      void finishWizard();
    } else if (current === 'name') {
      void saveThenAdvance();
    } else {
      advance();
    }
  }

  const canProceed =
    current === 'connect'
      ? hasPort(device)
      : current === 'name'
        ? machineName.trim().length > 0
        : true;

  const primaryLabel =
    current === 'done'
      ? finishing
        ? capturing
          ? 'Returning…'
          : 'Starting…'
        : capturing
          ? 'Back to your roast'
          : 'Start roasting'
      : current === 'name'
        ? saving
          ? 'Saving…'
          : 'Save & continue'
        : 'Continue';

  return (
    <div className="h-full w-full overflow-y-auto">
      <div className="mx-auto flex min-h-full w-full max-w-[780px] flex-col px-6 py-8 lg:px-10">
        {/* top bar */}
        <div className="flex items-center justify-between gap-4">
          <span className="text-[11px] font-semibold uppercase tracking-[0.16em] text-ink/40">
            Set up
            {mode === 'browser' ? ' · Browser demo' : ' · Droptime Logger'}
          </span>
          {current !== 'done' ? (
            <button
              type="button"
              onClick={() => void finishWizard()}
              className="dt-focus-ring text-xs font-semibold text-ink/45 hover:text-pine"
            >
              Skip setup
            </button>
          ) : null}
        </div>

        {/* progress */}
        <div className="mt-6">
          <Stepper
            steps={sequence.map((id) => ({ id, label: STEP_META[id].label }))}
            currentIndex={index}
          />
        </div>

        {/* title */}
        <h1 className="mt-8 text-2xl font-bold tracking-[-0.06em] text-pine">
          {STEP_META[current].title}
          {current === 'welcome' ? <span className="text-lemon">.</span> : null}
        </h1>

        {/* body */}
        <div className="mt-5 flex-1">
          {current === 'welcome' ? (
            <WelcomeStep
              mode={mode}
              path={path}
              onPathChange={setPath}
              unit={unit}
              onUnitChange={setUnit}
            />
          ) : current === 'connect' ? (
            <ConnectStep device={device} onChange={patchDevice} />
          ) : current === 'name' ? (
            <NameStep
              device={device}
              name={machineName}
              make={machineMake}
              onNameChange={(n) => {
                setMachineName(n);
                setNameError(null);
              }}
              onMakeChange={setMachineMake}
              error={nameError}
            />
          ) : current === 'import' ? (
            <ImportStep
              mode={mode}
              importedCount={importedCount}
              onImported={(uuids) => setImportedCount((c) => c + uuids.length)}
            />
          ) : (
            <DoneStep
              mode={mode}
              path={path}
              importedCount={importedCount}
              savedMachineName={savedMachineName}
              unit={unit}
            />
          )}
        </div>

        {/* footer nav */}
        <div className="mt-10 flex items-center justify-between gap-3 border-t border-line pt-5">
          {index > 0 ? (
            <Button variant="ghost" onClick={goBack} disabled={saving || finishing}>
              <CaretLeft size={15} weight="bold" /> Back
            </Button>
          ) : (
            <span />
          )}

          <Button variant="primary" onClick={onPrimary} disabled={!canProceed || saving || finishing}>
            {primaryLabel}
            {current === 'done' ? (
              <Rocket size={15} weight="bold" />
            ) : (
              <CaretRight size={15} weight="bold" />
            )}
          </Button>
        </div>
      </div>
    </div>
  );
}
