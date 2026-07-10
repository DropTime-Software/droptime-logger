/**
 * Step 2 (desktop only) — Connect your roaster. Scans serial ports, lets the
 * operator test a port (sniff → TC4 verdict + copy-pasteable device report),
 * optionally watch a live channel preview, and assign the bean/environment
 * probe channels, baud, and unit that become the machine's SourcePin.
 *
 * The §7.2 serial commands are fully implemented (serialport-backed sniffer,
 * preview, TC4 driver). Every device call still treats failure as a
 * first-class, non-blocking state ("couldn't scan / test / preview") that lets
 * the operator type a port and continue — `port_busy`, `port_error`, and a
 * silent rig are normal outcomes, never dead ends.
 */
import { useCallback, useEffect, useRef, useState } from 'react';
import { Channel } from '@tauri-apps/api/core';
import {
  ArrowsClockwise,
  Broadcast,
  CheckCircle,
  CircleNotch,
  ClipboardText,
  Copy,
  Keyboard,
  Star,
  WarningCircle,
} from '@phosphor-icons/react';
import { formatTemp } from '@droptime/roast-console';
import { ipc, asLoggerError } from '../../../bridge';
import type { PreviewEvent, SerialPortDto, SniffResultDto } from '../../../bridge';
import { Field } from '../../../components/ui';
import { SegmentedToggle } from '../../../features/appShell/controls';
import { BAUD_OPTIONS, type DeviceConfig } from '../machine';

/** TC4 exposes four logical channels; role pickers always offer 1..4. */
const TC4_CHANNELS = [1, 2, 3, 4] as const;

type ScanState =
  | { status: 'scanning' }
  | { status: 'ready'; ports: SerialPortDto[] }
  | { status: 'error'; message: string };

type SniffState =
  | { status: 'idle' }
  | { status: 'testing' }
  | { status: 'done'; result: SniffResultDto }
  | { status: 'error'; message: string };

type PreviewState =
  | { status: 'idle' }
  | { status: 'starting' }
  | { status: 'live' }
  | { status: 'error'; message: string };

export function ConnectStep({
  device,
  onChange,
}: {
  device: DeviceConfig;
  onChange: (patch: Partial<DeviceConfig>) => void;
}) {
  const [scan, setScan] = useState<ScanState>({ status: 'scanning' });
  const [manual, setManual] = useState(false);
  const [sniff, setSniff] = useState<SniffState>({ status: 'idle' });
  const [preview, setPreview] = useState<PreviewState>({ status: 'idle' });
  const [latest, setLatest] = useState<PreviewEvent | null>(null);
  const [copied, setCopied] = useState(false);

  const previewChannelRef = useRef<Channel<PreviewEvent> | null>(null);

  const stopPreview = useCallback(async () => {
    if (!previewChannelRef.current) return;
    previewChannelRef.current = null;
    setPreview({ status: 'idle' });
    setLatest(null);
    try {
      await ipc.stopPortPreview();
    } catch {
      /* best-effort; the engine may already be stopped */
    }
  }, []);

  const runScan = useCallback(async () => {
    await stopPreview();
    setScan({ status: 'scanning' });
    try {
      const ports = await ipc.listSerialPorts();
      const sorted = [...ports].sort(
        (a, b) => Number(b.likely) - Number(a.likely) || a.portName.localeCompare(b.portName),
      );
      setScan({ status: 'ready', ports: sorted });
    } catch (err) {
      setScan({ status: 'error', message: asLoggerError(err).message });
    }
  }, [stopPreview]);

  // Scan on mount; tear down any live preview on unmount.
  useEffect(() => {
    void runScan();
    return () => {
      void stopPreview();
    };
  }, [runScan, stopPreview]);

  function selectPort(portName: string) {
    if (portName === device.portName) return;
    void stopPreview();
    setSniff({ status: 'idle' });
    onChange({ portName });
  }

  async function testPort() {
    if (!device.portName.trim()) return;
    setSniff({ status: 'testing' });
    try {
      const result = await ipc.sniffPort({ portName: device.portName, baud: device.baud });
      setSniff({ status: 'done', result });
    } catch (err) {
      setSniff({ status: 'error', message: asLoggerError(err).message });
    }
  }

  async function copyReport(diagnostic: string) {
    try {
      await navigator.clipboard.writeText(diagnostic);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1800);
    } catch {
      /* clipboard unavailable — the report is still visible on screen */
    }
  }

  async function togglePreview() {
    if (previewChannelRef.current) {
      await stopPreview();
      return;
    }
    if (!device.portName.trim()) return;
    setPreview({ status: 'starting' });
    const channel = new Channel<PreviewEvent>();
    channel.onmessage = (ev) => {
      if (previewChannelRef.current !== channel) return;
      setLatest(ev);
      setPreview({ status: 'live' });
    };
    previewChannelRef.current = channel;
    try {
      await ipc.startPortPreview(
        { portName: device.portName, baud: device.baud, unit: device.unit },
        channel,
      );
    } catch (err) {
      previewChannelRef.current = null;
      setPreview({ status: 'error', message: asLoggerError(err).message });
    }
  }

  const hasPort = device.portName.trim().length > 0;

  return (
    <div className="flex flex-col gap-6">
      <p className="text-sm leading-relaxed text-ink/60">
        Plug your thermocouple board into USB, then pick it below. We only ever read from it.
      </p>

      {/* --- port picker --- */}
      <section>
        <div className="mb-2 flex items-center justify-between">
          <span className="text-[11px] font-semibold uppercase tracking-[0.14em] text-forest">
            Serial device
          </span>
          <button
            type="button"
            onClick={() => void runScan()}
            disabled={scan.status === 'scanning'}
            className="dt-focus-ring inline-flex items-center gap-1.5 text-xs font-semibold text-forest hover:underline disabled:opacity-50"
          >
            <ArrowsClockwise
              size={13}
              weight="bold"
              className={scan.status === 'scanning' ? 'animate-spin' : ''}
            />
            {scan.status === 'scanning' ? 'Scanning…' : 'Rescan'}
          </button>
        </div>

        {scan.status === 'scanning' ? (
          <Skeleton>Looking for connected devices…</Skeleton>
        ) : scan.status === 'error' ? (
          <NoticeCard tone="warn">
            We couldn't scan for connected devices right now. Reconnect the board and rescan,
            or enter the port name yourself.
          </NoticeCard>
        ) : scan.ports.length === 0 ? (
          <NoticeCard tone="neutral">
            No serial devices found. Make sure the board is plugged in and its driver is
            installed, then rescan — or enter the port name yourself.
          </NoticeCard>
        ) : (
          <div className="grid gap-2 sm:grid-cols-2">
            {scan.ports.map((port) => (
              <PortRow
                key={port.portName}
                port={port}
                active={!manual && port.portName === device.portName}
                onSelect={() => selectPort(port.portName)}
              />
            ))}
          </div>
        )}

        <button
          type="button"
          onClick={() => {
            setManual((m) => !m);
            if (!manual) onChange({ portName: '' });
          }}
          className="dt-focus-ring mt-2 inline-flex items-center gap-1.5 text-xs font-medium text-ink/55 hover:text-pine"
        >
          <Keyboard size={14} weight="bold" />
          {manual ? 'Choose from the list instead' : 'Enter a port name manually'}
        </button>

        {manual ? (
          <div className="mt-2">
            <input
              className="dt-input dt-focus-ring"
              value={device.portName}
              placeholder="e.g. /dev/tty.usbserial-1420 or COM3"
              autoComplete="off"
              spellCheck={false}
              onChange={(e) => onChange({ portName: e.target.value })}
            />
          </div>
        ) : null}
      </section>

      {/* --- test + preview (only meaningful once a port is chosen) --- */}
      {hasPort ? (
        <section className="flex flex-col gap-4">
          <div className="flex flex-wrap items-center gap-2.5">
            <button
              type="button"
              onClick={() => void testPort()}
              disabled={sniff.status === 'testing'}
              className="dt-btn dt-btn-ghost dt-focus-ring h-10 px-4 text-sm"
            >
              {sniff.status === 'testing' ? (
                <>
                  <CircleNotch size={15} weight="bold" className="animate-spin" /> Testing…
                </>
              ) : (
                <>
                  <CheckCircle size={15} weight="bold" /> Test connection
                </>
              )}
            </button>
            <button
              type="button"
              onClick={() => void togglePreview()}
              disabled={preview.status === 'starting'}
              className="dt-btn dt-btn-ghost dt-focus-ring h-10 px-4 text-sm"
            >
              <Broadcast
                size={15}
                weight="bold"
                className={preview.status === 'live' ? 'text-grass' : ''}
              />
              {previewChannelRef.current ? 'Stop preview' : 'Live preview'}
            </button>
          </div>

          {sniff.status === 'done' ? <SniffResult result={sniff.result} copied={copied} onCopy={copyReport} /> : null}
          {sniff.status === 'error' ? (
            <NoticeCard tone="warn">
              Couldn't test this port right now ({sniff.message}). You can still set the channels
              below and continue.
            </NoticeCard>
          ) : null}

          {preview.status === 'error' ? (
            <NoticeCard tone="warn">
              Live preview isn't available right now ({preview.message}). Set the channels from
              your board's wiring and continue.
            </NoticeCard>
          ) : null}
          {preview.status === 'starting' ? <Skeleton>Opening the port…</Skeleton> : null}
          {preview.status === 'live' ? <PreviewReadout latest={latest} unit={device.unit} /> : null}
        </section>
      ) : null}

      {/* --- channel roles + link settings --- */}
      <section className="flex flex-col gap-5 border-t border-line pt-6">
        <Field
          label="Bean probe channel (BT)"
          hint="Which TC4 channel reads bean temperature. Usually channel 1."
        >
          <ChannelPicker
            value={device.btChannel}
            unit={device.unit}
            onChange={(ch) => {
              if (ch != null) onChange({ btChannel: ch });
            }}
            reading={latest ? channelReading(latest, device.btChannel) : undefined}
          />
        </Field>
        <Field
          label="Environment probe channel (ET)"
          hint="Optional — the exhaust/air probe. Leave off if you only run one probe."
        >
          <ChannelPicker
            value={device.etChannel}
            unit={device.unit}
            allowNone
            onChange={(ch) => onChange({ etChannel: ch })}
            reading={
              device.etChannel != null && latest
                ? channelReading(latest, device.etChannel)
                : undefined
            }
          />
        </Field>

        <div className="grid gap-5 sm:grid-cols-2">
          <Field label="Baud rate" hint="TC4 firmware default is 115200.">
            <SegmentedToggle
              ariaLabel="Baud rate"
              value={String(device.baud)}
              options={BAUD_OPTIONS.map((b) => ({ value: String(b), label: String(b) }))}
              onChange={(v) => onChange({ baud: Number(v) })}
            />
          </Field>
          <Field label="Board reports temperatures in" hint="How the board sends readings; stored as °F.">
            <SegmentedToggle
              ariaLabel="Device unit"
              value={device.unit}
              options={[
                { value: 'F', label: '°F' },
                { value: 'C', label: '°C' },
              ]}
              onChange={(v) => onChange({ unit: v })}
            />
          </Field>
        </div>
      </section>
    </div>
  );
}

// ---------------------------------------------------------------------------

function PortRow({
  port,
  active,
  onSelect,
}: {
  port: SerialPortDto;
  active: boolean;
  onSelect: () => void;
}) {
  const subtitle = [port.product ?? port.manufacturer, chipLabel(port.chip)]
    .filter(Boolean)
    .join(' · ');
  return (
    <button
      type="button"
      onClick={onSelect}
      aria-pressed={active}
      className="dt-focus-ring flex items-center justify-between gap-3 rounded-xl border px-4 py-3 text-left transition"
      style={{
        borderColor: active ? 'var(--color-forest)' : 'var(--color-line)',
        background: active ? 'rgba(218,246,152,0.35)' : '#ffffff',
        boxShadow: '0px 0px 4px 0px rgba(0,0,0,0.06)',
      }}
    >
      <span className="min-w-0">
        <span className="flex items-center gap-1.5">
          <span className="truncate font-semibold text-pine">{port.portName}</span>
          {port.likely ? (
            <span
              title="Looks like a TC4 rig"
              className="inline-flex items-center gap-0.5 rounded-full bg-mint px-1.5 py-0.5 text-[10px] font-semibold text-forest"
            >
              <Star size={9} weight="fill" /> likely
            </span>
          ) : null}
        </span>
        {subtitle ? <span className="mt-0.5 block truncate text-xs text-ink/50">{subtitle}</span> : null}
      </span>
      <span
        className="h-4 w-4 shrink-0 rounded-full border"
        style={{
          borderColor: active ? 'var(--color-forest)' : 'var(--color-line2)',
          background: active ? 'var(--color-forest)' : 'transparent',
        }}
      />
    </button>
  );
}

function SniffResult({
  result,
  copied,
  onCopy,
}: {
  result: SniffResultDto;
  copied: boolean;
  onCopy: (diagnostic: string) => void;
}) {
  const good = result.verdict === 'tc4';
  return (
    <div
      className="rounded-xl border p-4"
      style={{
        borderColor: good ? 'rgba(60,140,90,0.4)' : 'var(--color-line2)',
        background: good ? 'rgba(60,140,90,0.06)' : 'rgba(232,164,77,0.07)',
      }}
    >
      <div className="flex items-center gap-2">
        {good ? (
          <CheckCircle size={18} weight="fill" className="text-grass" />
        ) : (
          <WarningCircle size={18} weight="fill" className="text-amber" />
        )}
        <span className="text-sm font-semibold text-pine">{verdictLabel(result.verdict)}</span>
        {result.channels && result.channels.length > 0 ? (
          <span className="text-xs text-ink/50">
            {result.channels.length} channel{result.channels.length === 1 ? '' : 's'} seen
          </span>
        ) : null}
      </div>
      {result.rawFrames.length > 0 ? (
        <pre className="mt-3 max-h-24 overflow-auto rounded-lg bg-white/70 p-2 text-[11px] leading-relaxed text-ink/70">
          {result.rawFrames.slice(0, 4).join('\n')}
        </pre>
      ) : null}
      <button
        type="button"
        onClick={() => onCopy(result.diagnostic)}
        className="dt-focus-ring mt-3 inline-flex items-center gap-1.5 text-xs font-semibold text-forest hover:underline"
      >
        {copied ? <ClipboardText size={13} weight="bold" /> : <Copy size={13} weight="bold" />}
        {copied ? 'Report copied' : 'Copy device report'}
      </button>
    </div>
  );
}

function PreviewReadout({ latest, unit }: { latest: PreviewEvent | null; unit: 'F' | 'C' }) {
  return (
    <div className="rounded-xl border border-line bg-white p-4" style={{ boxShadow: '0px 0px 4px rgba(0,0,0,0.06)' }}>
      <div className="mb-2 flex items-center gap-1.5 text-[11px] font-semibold uppercase tracking-[0.12em] text-grass">
        <span className="inline-block h-2 w-2 animate-pulse rounded-full bg-grass" /> Live
      </div>
      {latest ? (
        <div className="flex flex-wrap gap-2">
          {latest.channels.map((value, i) => (
            <span
              key={i}
              className="rounded-lg bg-mint px-2.5 py-1.5 text-sm tabular-nums text-pine"
            >
              <span className="mr-1 text-[10px] font-semibold uppercase text-forest/60">Ch{i + 1}</span>
              {value == null ? '—' : formatTemp(value, unit, 0)}
            </span>
          ))}
        </div>
      ) : (
        <p className="text-sm text-ink/50">Waiting for the first frame…</p>
      )}
    </div>
  );
}

function ChannelPicker({
  value,
  onChange,
  allowNone,
  reading,
  unit,
}: {
  value: number | null;
  onChange: (channel: number | null) => void;
  allowNone?: boolean;
  reading?: number;
  unit: 'F' | 'C';
}) {
  return (
    <div className="flex flex-wrap items-center gap-2">
      {allowNone ? (
        <ChannelChip label="None" active={value == null} onClick={() => onChange(null)} />
      ) : null}
      {TC4_CHANNELS.map((ch) => (
        <ChannelChip
          key={ch}
          label={`Ch ${ch}`}
          active={value === ch}
          onClick={() => onChange(ch)}
        />
      ))}
      {reading != null ? (
        <span className="ml-1 text-xs tabular-nums text-ink/50">reading {formatTemp(reading, unit, 0)}</span>
      ) : null}
    </div>
  );
}

function ChannelChip({
  label,
  active,
  onClick,
}: {
  label: string;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-pressed={active}
      className="dt-focus-ring rounded-full border px-3 py-1.5 text-xs font-semibold transition"
      style={{
        borderColor: active ? 'var(--color-forest)' : 'var(--color-line2)',
        background: active ? 'var(--color-lemon)' : '#ffffff',
        color: active ? 'var(--color-forest)' : 'rgba(36,36,36,0.6)',
      }}
    >
      {label}
    </button>
  );
}

function NoticeCard({ tone, children }: { tone: 'warn' | 'neutral'; children: React.ReactNode }) {
  return (
    <div
      className="rounded-xl border px-4 py-3 text-sm leading-relaxed"
      style={{
        borderColor: tone === 'warn' ? 'rgba(232,164,77,0.4)' : 'var(--color-line)',
        background: tone === 'warn' ? 'rgba(232,164,77,0.07)' : 'var(--color-mint)',
        color: 'rgba(36,36,36,0.7)',
      }}
    >
      {children}
    </div>
  );
}

function Skeleton({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex items-center gap-2 rounded-xl border border-line bg-white px-4 py-3 text-sm text-ink/50">
      <CircleNotch size={15} weight="bold" className="animate-spin" />
      {children}
    </div>
  );
}

function channelReading(latest: PreviewEvent, channel: number): number | undefined {
  const v = latest.channels[channel - 1];
  return v == null ? undefined : v;
}

function chipLabel(chip?: SerialPortDto['chip']): string | undefined {
  if (!chip || chip === 'other') return undefined;
  return chip.toUpperCase();
}

function verdictLabel(verdict: SniffResultDto['verdict']): string {
  switch (verdict) {
    case 'tc4':
      return 'TC4 board detected — you’re good to go';
    case 'silent':
      return 'Port opened, but no data yet';
    default:
      return 'Connected, but this doesn’t look like TC4';
  }
}
