/**
 * Setup-wizard step machine + SourcePin assembly — a pure, side-effect-free
 * module (no React, no IPC). Everything here is deterministic and trivially
 * testable by inspection; the screen shell (`index.tsx`) owns all effects.
 *
 * The step sequence branches on the runtime mode and the chosen path:
 *  - browser demo has no serial hardware, so it skips straight to import;
 *  - the desktop "connect a roaster" path threads through serial + naming;
 *  - the desktop "explore" path mirrors the browser flow.
 */
import type { AppMode, SourcePin } from '../../bridge';

export type WizardStepId = 'welcome' | 'connect' | 'name' | 'import' | 'done';

/** What the operator picked on the welcome step. */
export type WizardPath = 'device' | 'demo';

/** Human-facing copy for the stepper + step headers. */
export const STEP_META: Record<WizardStepId, { label: string; title: string }> = {
  welcome: { label: 'Welcome', title: 'Welcome to Droptime Logger' },
  connect: { label: 'Connect', title: 'Connect your roaster' },
  name: { label: 'Save it', title: 'Save your roaster' },
  import: { label: 'Import', title: 'Bring your roast history' },
  done: { label: 'Done', title: "You're all set" },
};

/**
 * The ordered steps for a given mode + path. Browser mode never has a serial
 * device, so it collapses to the same short flow as the desktop "explore" path.
 */
export function stepSequence(mode: AppMode, path: WizardPath): WizardStepId[] {
  if (mode === 'browser') return ['welcome', 'import', 'done'];
  return path === 'device'
    ? ['welcome', 'connect', 'name', 'import', 'done']
    : ['welcome', 'import', 'done'];
}

/** Clamp an index into a sequence to a valid position. */
export function clampIndex(index: number, length: number): number {
  if (length <= 0) return 0;
  return Math.min(Math.max(index, 0), length - 1);
}

// ---------------------------------------------------------------------------
// Device configuration → SourcePin (CONTRACTS.md §5 / §7.2)
// ---------------------------------------------------------------------------

/**
 * Everything the connect step collects about a TC4 rig. `portName` is empty
 * until a port is picked or typed; channels are 1-based TC4 logical channels.
 */
export interface DeviceConfig {
  portName: string;
  /** TC4 default */
  baud: number;
  /** 1-based; BT is required */
  btChannel: number;
  /** 1-based; ET is optional (null = not wired) */
  etChannel: number | null;
  unit: 'F' | 'C';
}

export const DEFAULT_DEVICE_CONFIG: DeviceConfig = {
  portName: '',
  baud: 115200,
  btChannel: 1,
  etChannel: 2,
  unit: 'F',
};

/** Common TC4 baud rates offered in the connect step. */
export const BAUD_OPTIONS: readonly number[] = [9600, 19200, 57600, 115200];

/**
 * Assemble the persisted `SourcePin` (machines_local.source_pin / start_session
 * sourcePin) from the collected device config. `etChannel` is omitted entirely
 * when unwired so the JSON matches the contract's optional shape.
 */
export function assembleSourcePin(cfg: DeviceConfig): SourcePin {
  const pin: SourcePin = {
    sourceId: `tc4:${cfg.portName}`,
    baud: cfg.baud,
    btChannel: cfg.btChannel,
    unit: cfg.unit,
  };
  if (cfg.etChannel != null) pin.etChannel = cfg.etChannel;
  return pin;
}

/** The `sourcePinJson` string for `ipc.saveMachine`. */
export function sourcePinJson(cfg: DeviceConfig): string {
  return JSON.stringify(assembleSourcePin(cfg));
}

/** A device config is savable once it points at a real port. */
export function hasPort(cfg: DeviceConfig): boolean {
  return cfg.portName.trim().length > 0;
}
