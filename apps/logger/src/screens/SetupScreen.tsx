import { useEffect, useMemo, useState } from 'react';
import type { AppMode, MachineDto, SourceInfo, SourcePin } from '../bridge';
import { ipc } from '../bridge';
import { browserFixtureSources, useSession, type StartConfig } from '../state/SessionProvider';
import { Button, Field, TextInput } from '../components/ui';
import { CoffeeMachineFields } from '../features/library/CoffeeMachineFields';
import { ReferenceSection } from '../features/reference/ReferenceSection';

interface SourceOption {
  id: string;
  label: string;
  sublabel?: string;
  fixtureName?: string;
  coffeeName?: string;
  chargeWeightLb?: number;
  /** replay = bundled fixture (browser sim / Rust replay); device = saved TC4 roaster */
  kind: 'replay' | 'device';
  /** device only — the machines_local SourcePin the engine starts with (§7.2). */
  sourcePin?: SourcePin;
  /** device only — links the roast to the library row. */
  machineLocalId?: number;
  machineName?: string;
}

export function SetupScreen({ mode }: { mode: AppMode }) {
  const { state, actions } = useSession();

  const [sources, setSources] = useState<SourceOption[] | null>(mode === 'tauri' ? null : []);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [selectedId, setSelectedId] = useState<string | undefined>();
  const [speed, setSpeed] = useState(6);
  const [coffeeName, setCoffeeName] = useState('');
  const [chargeWeight, setChargeWeight] = useState('');
  const [machineName, setMachineName] = useState('');
  // Library links resolved by the CoffeeMachineFields feature (undefined = free text).
  const [coffeeLocalId, setCoffeeLocalId] = useState<number | undefined>();
  const [machineLocalId, setMachineLocalId] = useState<number | undefined>();
  const [starting, setStarting] = useState(false);
  const [startError, setStartError] = useState<string | null>(null);
  const [touchedCoffee, setTouchedCoffee] = useState(false);
  const [touchedWeight, setTouchedWeight] = useState(false);

  // Load selectable sources.
  useEffect(() => {
    if (mode === 'browser') {
      const opts: SourceOption[] = browserFixtureSources().map(({ key, fixture }) => ({
        id: `sim:${key}`,
        label: fixture.name,
        sublabel: fixture.description,
        fixtureName: key,
        coffeeName: fixture.markers.coffeeName,
        chargeWeightLb: fixture.markers.chargeWeightLb,
        kind: 'replay',
      }));
      setSources(opts);
      return;
    }
    let cancelled = false;
    // Saved TC4 roasters (with a parseable tc4: pin) become live device sources;
    // listMachines failing is non-fatal — the replay list still renders.
    Promise.all([ipc.listSources(), ipc.listMachines().catch(() => [] as MachineDto[])])
      .then(([list, machines]: [SourceInfo[], MachineDto[]]) => {
        if (cancelled) return;
        const deviceOpts: SourceOption[] = [];
        for (const m of machines) {
          if (m.archived || !m.sourcePinJson) continue;
          let pin: SourcePin;
          try {
            pin = JSON.parse(m.sourcePinJson) as SourcePin;
          } catch {
            continue;
          }
          if (!pin || typeof pin.sourceId !== 'string' || !pin.sourceId.startsWith('tc4:')) {
            continue;
          }
          deviceOpts.push({
            // Key on the machine row, NOT pin.sourceId — two machines pinned to the
            // same port must not collide as React keys / selection ids.
            id: `machine:${m.id}`,
            label: m.name,
            sublabel: `Live roaster — ${pin.sourceId.slice(4)}`,
            kind: 'device',
            sourcePin: pin,
            machineLocalId: m.id,
            machineName: m.name,
          });
        }
        const replayOpts: SourceOption[] = list.map((s) => ({
          id: s.id,
          label: s.label,
          sublabel: s.kind,
          kind: 'replay',
        }));
        setSources([...deviceOpts, ...replayOpts]);
      })
      .catch((err) => {
        if (cancelled) return;
        setLoadError(err instanceof Error ? err.message : 'Could not load sources.');
        setSources([]);
      });
    return () => {
      cancelled = true;
    };
  }, [mode]);

  const selected = useMemo(
    () => sources?.find((s) => s.id === selectedId),
    [sources, selectedId],
  );

  // Preselect the first saved live roaster so a TC4 owner who just finished the
  // wizard lands ready to log — rather than stranded among replay-only options.
  useEffect(() => {
    if (!sources || selectedId != null) return;
    const firstDevice = sources.find((s) => s.kind === 'device');
    if (firstDevice) {
      setSelectedId(firstDevice.id);
      setMachineName(firstDevice.machineName ?? '');
      setMachineLocalId(firstDevice.machineLocalId);
    }
  }, [sources, selectedId]);

  function selectSource(opt: SourceOption) {
    setSelectedId(opt.id);
    if (!touchedCoffee) setCoffeeName(opt.coffeeName ?? '');
    if (!touchedWeight) setChargeWeight(opt.chargeWeightLb != null ? String(opt.chargeWeightLb) : '');
    if (opt.kind === 'device') {
      // Link the roast to the saved library row and surface its name in the field.
      setMachineName(opt.machineName ?? '');
      setMachineLocalId(opt.machineLocalId);
    }
  }

  async function start() {
    if (!selected) return;
    setStarting(true);
    setStartError(null);
    const chargeWeightLb = chargeWeight.trim() ? Number(chargeWeight) : undefined;
    const isDevice = selected.kind === 'device';
    const cfg: StartConfig = {
      // Device options carry the ENGINE source id (`tc4:<port>`) in the pin;
      // SessionProvider derives kind/auto-charge from the tc4: prefix.
      sourceId: selected.sourcePin?.sourceId ?? selected.id,
      sourceLabel: selected.label,
      fixtureName: selected.fixtureName,
      // Live hardware runs real-time; the replay-speed slider only applies to fixtures.
      speed: isDevice ? 1 : speed,
      coffeeName: coffeeName.trim() || undefined,
      chargeWeightLb: Number.isFinite(chargeWeightLb) ? chargeWeightLb : undefined,
      machineName: machineName.trim() || undefined,
      coffeeLocalId,
      machineLocalId,
      referenceRoastUuid: state.reference?.roastUuid,
      sourcePin: selected.sourcePin,
    };
    try {
      await actions.startRoast(cfg);
    } catch (err) {
      setStartError(err instanceof Error ? err.message : 'Could not start the session.');
      setStarting(false);
    }
  }

  const selectedIsDevice = selected?.kind === 'device';
  const hasDeviceOption = sources?.some((s) => s.kind === 'device') ?? false;
  const sourceHint =
    mode !== 'tauri'
      ? undefined
      : hasDeviceOption
        ? 'Pick a saved roaster to log live, or replay a bundled profile.'
        : 'Re-run setup from Settings to connect a roaster, or replay a bundled profile below.';

  return (
    <div className="h-full w-full overflow-y-auto">
      <div className="mx-auto flex min-h-full w-full max-w-[1600px] flex-col gap-7 px-6 py-8 lg:px-10">
        <div className="flex flex-wrap items-end justify-between gap-4">
          <div>
            <h1 className="text-2xl font-bold tracking-[-0.06em] text-pine">New roast</h1>
            <p className="mt-1 text-sm text-ink/60">
              {mode === 'browser'
                ? 'Replay a bundled profile to explore the live console. Nothing is saved in browser demo mode.'
                : 'Pick a source and log a roast. Everything is captured locally, first.'}
            </p>
          </div>
          <Button variant="primary" onClick={start} disabled={!selected || starting}>
            {starting ? 'Starting…' : 'Start roasting'}
          </Button>
        </div>

        <Field label="Source" hint={sourceHint}>
          {sources === null ? (
            <p className="text-sm text-ink/50">Loading sources…</p>
          ) : sources.length === 0 ? (
            <p className="text-sm text-ink/50">{loadError ?? 'No sources available.'}</p>
          ) : (
            <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
              {sources.map((opt) => {
                const active = opt.id === selectedId;
                return (
                  <button
                    key={opt.id}
                    onClick={() => selectSource(opt)}
                    className="dt-focus-ring flex items-start justify-between gap-3 rounded-2xl border px-5 py-4 text-left transition"
                    style={{
                      borderColor: active ? 'var(--color-forest)' : 'var(--color-line)',
                      background: active ? 'rgba(218,246,152,0.35)' : '#ffffff',
                      boxShadow: '0px 0px 4px 0px rgba(0,0,0,0.08)',
                    }}
                  >
                    <span className="min-w-0">
                      <span className="block truncate font-semibold text-pine">{opt.label}</span>
                      {opt.sublabel ? (
                        <span className="mt-0.5 block text-xs leading-relaxed text-ink/50">
                          {opt.sublabel}
                        </span>
                      ) : null}
                    </span>
                    <span
                      className="mt-0.5 h-4 w-4 shrink-0 rounded-full border"
                      style={{
                        borderColor: active ? 'var(--color-forest)' : 'var(--color-line2)',
                        background: active ? 'var(--color-forest)' : 'transparent',
                      }}
                    />
                  </button>
                );
              })}
            </div>
          )}
        </Field>

        <div className="grid grid-cols-1 gap-x-8 gap-y-6 lg:grid-cols-2 xl:grid-cols-4">
          {/* Replay speed applies to bundled fixtures only — live rigs run real-time. */}
          {!selectedIsDevice ? (
            <Field label={`Replay speed — ${speed}×`}>
              <input
                type="range"
                min={1}
                max={20}
                step={1}
                value={speed}
                onChange={(e) => setSpeed(Number(e.target.value))}
                className="dt-focus-ring w-full accent-forest"
              />
              <div className="mt-1 flex justify-between text-[11px] text-ink/45">
                <span>1× real-time</span>
                <span>20× fast</span>
              </div>
            </Field>
          ) : null}
          <Field label="Charge weight (lb)" htmlFor="weight">
            <TextInput
              id="weight"
              type="number"
              inputMode="decimal"
              min="0"
              step="0.1"
              placeholder="e.g. 2.0"
              value={chargeWeight}
              onChange={(e) => {
                setTouchedWeight(true);
                setChargeWeight(e.target.value);
              }}
            />
          </Field>

          {/* Library-backed coffee/machine pickers (plain inputs in browser mode). */}
          <CoffeeMachineFields
            coffeeName={coffeeName}
            onCoffeeNameChange={(name) => {
              setTouchedCoffee(true);
              setCoffeeName(name);
            }}
            machineName={machineName}
            onMachineNameChange={setMachineName}
            coffeeLocalId={coffeeLocalId}
            onCoffeeLocalIdChange={setCoffeeLocalId}
            machineLocalId={machineLocalId}
            onMachineLocalIdChange={setMachineLocalId}
          />
        </div>

        {/* Background-replay reference picker. */}
        <ReferenceSection />

        {startError ? <p className="text-sm text-red-600">{startError}</p> : null}

        <div className="mt-auto flex justify-end pt-2">
          <Button variant="primary" onClick={start} disabled={!selected || starting}>
            {starting ? 'Starting…' : 'Start roasting'}
          </Button>
        </div>
      </div>
    </div>
  );
}
