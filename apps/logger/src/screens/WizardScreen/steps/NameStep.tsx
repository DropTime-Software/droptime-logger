/**
 * Step 3 (desktop only) — Save your roaster. Names the machine and shows the
 * SourcePin that gets stored with it (machines_local.source_pin, CONTRACTS §5).
 * The actual `ipc.saveMachine` call is owned by the wizard shell's Continue
 * handler so the save is tied to progression; this step only collects input and
 * surfaces any save error.
 */
import { Field, TextInput } from '../../../components/ui';
import { assembleSourcePin, hasPort, type DeviceConfig } from '../machine';

export function NameStep({
  device,
  name,
  make,
  onNameChange,
  onMakeChange,
  error,
}: {
  device: DeviceConfig;
  name: string;
  make: string;
  onNameChange: (name: string) => void;
  onMakeChange: (make: string) => void;
  error: string | null;
}) {
  const pin = assembleSourcePin(device);
  const chips: Array<{ label: string; value: string }> = [
    { label: 'Port', value: hasPort(device) ? device.portName : 'not set' },
    { label: 'Bean (BT)', value: `Ch ${device.btChannel}` },
    { label: 'Env (ET)', value: device.etChannel != null ? `Ch ${device.etChannel}` : 'off' },
    { label: 'Baud', value: String(device.baud) },
    { label: 'Reports', value: `°${device.unit}` },
  ];

  return (
    <div className="flex flex-col gap-6">
      <p className="text-sm leading-relaxed text-ink/60">
        Give this roaster a name so you can pick it in one click next time — its connection
        settings are saved with it.
      </p>

      <div className="grid gap-5 sm:grid-cols-2">
        <Field label="Roaster name" htmlFor="wizard-machine-name" hint="What you call it day to day.">
          <TextInput
            id="wizard-machine-name"
            value={name}
            placeholder="e.g. Shop Loring"
            autoFocus
            onChange={(e) => onNameChange(e.target.value)}
          />
        </Field>
        <Field label="Make / model" htmlFor="wizard-machine-make" hint="Optional.">
          <TextInput
            id="wizard-machine-make"
            value={make}
            placeholder="e.g. Loring S15 Falcon"
            onChange={(e) => onMakeChange(e.target.value)}
          />
        </Field>
      </div>

      <div className="rounded-2xl border border-line bg-mint/60 p-4">
        <div className="mb-2.5 text-[11px] font-semibold uppercase tracking-[0.12em] text-forest">
          Connection saved with this roaster
        </div>
        <div className="flex flex-wrap gap-2">
          {chips.map((chip) => (
            <span
              key={chip.label}
              className="inline-flex items-center gap-1.5 rounded-lg bg-white px-2.5 py-1.5 text-xs"
              style={{ boxShadow: '0px 0px 3px rgba(0,0,0,0.08)' }}
            >
              <span className="font-semibold uppercase tracking-wide text-forest/60">{chip.label}</span>
              <span className="tabular-nums text-pine">{chip.value}</span>
            </span>
          ))}
        </div>
        <p className="mt-3 font-mono text-[11px] leading-relaxed text-ink/45">
          {pin.sourceId}
        </p>
      </div>

      {error ? <p className="text-sm text-red-600">{error}</p> : null}
    </div>
  );
}
