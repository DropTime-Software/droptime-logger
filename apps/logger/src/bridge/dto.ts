/**
 * IPC DTOs for the Tauri command surface (CONTRACTS.md §2).
 *
 * DTO field names mirror the shared contract (packages/roast-console/src/types.ts)
 * and serialize camelCase via serde `rename_all` on the Rust side. Where a DTO is
 * structurally identical to a contract type we alias it directly so the two stay
 * locked together.
 */
import type {
  LiveSample,
  RoastEvent,
  RoastEventKind,
  RoastMarkersSec,
  RoastSummary,
  SourceInfo,
  SourceStatusKind,
} from '@droptime/roast-console';

/**
 * CONTRACTS.md §6 — emitted whenever the BACKEND changes canonical markers
 * outside a `mark_event` call (v0.1.0: auto CHARGE/DROP, `auto: true`). The UI
 * applies it exactly like a `mark_event` response; `undo_event` works unchanged.
 */
export interface MarkerEvent {
  type: 'marker';
  kind: RoastEventKind;
  auto: boolean;
  atSessionSec: number;
  markers: RoastMarkersDto;
}

/** Ordered stream payload delivered on the `Channel<SampleEvent>` (CONTRACTS.md §2 + §6). */
export type SampleEvent =
  | {
      type: 'sample';
      seq: number;
      sessionSec: number;
      btF: number;
      etF?: number;
      ambientF?: number;
      heater?: number;
      fan?: number;
      drum?: number;
    }
  | {
      type: 'status';
      kind: SourceStatusKind;
      message?: string;
      atSessionSec: number;
    }
  | MarkerEvent;

/** `roasts` markers as returned by mark/undo — identical to the shared contract. */
export type RoastMarkersDto = RoastMarkersSec;

/** Row shape from list_roasts / finish_session / get_roast.summary. */
export type RoastSummaryDto = RoastSummary;

/** Sample row from get_roast. */
export type SampleDto = LiveSample;

/** Raw event tap from get_roast. */
export type EventDto = RoastEvent;

export interface StartSessionMeta {
  machineLocalId?: number;
  coffeeName?: string;
  chargeWeightLb?: number;
  /** v0.1.0 (§5/§8): optional link into coffees_local. */
  coffeeLocalId?: number;
  /** v0.1.0 (§5/§8): background-replay reference roast used, if any. */
  referenceRoastUuid?: string;
}

/**
 * `machines_local.source_pin` JSON + `start_session.sourcePin` (CONTRACTS §5).
 * `btChannel`/`etChannel` are 1-based TC4 logical channels; `etChannel` optional.
 */
export interface SourcePin {
  /** e.g. `tc4:<portName>` */
  sourceId: string;
  /** default 115200 */
  baud?: number;
  /** default 1 */
  btChannel?: number;
  etChannel?: number;
  /** default 'F' */
  unit?: 'F' | 'C';
}

export interface StartSessionArgs {
  sourceId: string;
  speed?: number;
  meta: StartSessionMeta;
  /** v0.1.0 (§7.2): required for `tc4:` sources; ignored for replay. */
  sourcePin?: SourcePin;
}

export interface StartSessionResult {
  roastUuid: string;
}

export interface ResumeSessionResult {
  resumedFromSeq: number;
}

export interface MarkEventArgs {
  roastUuid: string;
  kind: RoastEventKind;
  sessionSec?: number;
  note?: string;
}

export interface UndoEventArgs {
  roastUuid: string;
  kind: RoastEventKind;
}

export interface FinishSessionArgs {
  roastUuid: string;
  dropWeightLb?: number;
  notes?: string;
}

export interface RecoveryDto {
  roastUuid: string;
  startedWallMs: number;
  lastSeq: number;
  lastSessionSec: number;
  meta: StartSessionMeta & { machineName?: string };
}

export interface GetRoastResult {
  summary: RoastSummaryDto;
  samples: SampleDto[];
  events: EventDto[];
}

// ---------------------------------------------------------------------------
// v0.1.0 expansion DTOs (CONTRACTS.md §7)
// ---------------------------------------------------------------------------

/** One row of `list_serial_ports` (§7.2). `likely` = looks like a TC4 rig. */
export interface SerialPortDto {
  portName: string;
  vid?: number;
  pid?: number;
  manufacturer?: string;
  product?: string;
  serialNumber?: string;
  chip?: 'ftdi' | 'cp210x' | 'ch340' | 'other';
  likely: boolean;
}

export interface SniffPortArgs {
  portName: string;
  /** default 115200 */
  baud?: number;
  /** default 3000 */
  durationMs?: number;
}

/**
 * `sniff_port` result (§7.2). `diagnostic` is the copy-pasteable device-report
 * block (port, VID/PID, baud, frames) for the GitHub issue template.
 */
export interface SniffResultDto {
  verdict: 'tc4' | 'unknown' | 'silent';
  rawFrames: string[];
  channels?: number[];
  diagnostic: string;
}

export interface StartPortPreviewArgs {
  portName: string;
  /** default 115200 */
  baud?: number;
  /** default 'F' */
  unit?: 'F' | 'C';
}

/** ~1Hz frame on `Channel<PreviewEvent>` (§7.2); 1-based TC4 channel order. */
export interface PreviewEvent {
  channels: Array<number | null>;
  ambientF?: number;
  atMs: number;
}

/** Per-file parse-only preview (§7.1) — never writes. */
export interface ImportPreviewDto {
  path: string;
  ok: boolean;
  error?: string;
  coffeeName?: string;
  startedWallMs?: number;
  durationSec?: number;
  sampleCount?: number;
  markers?: RoastMarkersDto;
}

/** `import_roasts` result (§7.1). */
export interface ImportResultDto {
  imported: number;
  roastUuids: string[];
  failed: Array<{ path: string; error: string }>;
}

export type ExportFormat = 'alog' | 'csv' | 'json';

export interface ExportRoastArgs {
  roastUuid: string;
  format: ExportFormat;
  /** Chosen by the frontend via the dialog plugin. */
  destPath: string;
}

/** `machines_local` row (§7.3). Machines are never hard-deleted. */
export interface MachineDto {
  id: number;
  name: string;
  make?: string;
  /** SourcePin JSON (§5 shape), stored verbatim. */
  sourcePinJson?: string;
  archived: boolean;
}

export interface SaveMachineArgs {
  /** absent = create, present = update */
  id?: number;
  name: string;
  make?: string;
  sourcePinJson?: string;
}

/** `coffees_local` row (§7.3). */
export interface CoffeeDto {
  id: number;
  name: string;
  origin?: string;
  process?: string;
  notes?: string;
  archived: boolean;
}

export interface SaveCoffeeArgs {
  /** absent = create, present = update */
  id?: number;
  name: string;
  origin?: string;
  process?: string;
  notes?: string;
}

/** `update_roast` patch (§7.3): absent = unchanged, explicit `null` = clear. */
export interface RoastPatch {
  coffeeName?: string | null;
  coffeeLocalId?: number | null;
  machineLocalId?: number | null;
  chargeWeightLb?: number | null;
  dropWeightLb?: number | null;
  notes?: string | null;
}

/**
 * `update_roast_markers` patch (§7.3): absent = unchanged, `null` = clear.
 * Charge is NOT editable in v0.1.0; temps recompute from samples backend-side.
 */
export interface MarkerPatch {
  turningPointSec?: number | null;
  dryEndSec?: number | null;
  fcStartSec?: number | null;
  fcEndSec?: number | null;
  dropSec?: number | null;
}

/** `get_app_info` result (§7.4). */
export interface AppInfo {
  version: string;
  os: string;
  arch: string;
}

/** `check_for_update` result (§7.4) — notice-only in v0.1.0. */
export interface UpdateInfo {
  currentVersion: string;
  latestVersion: string;
  url: string;
  isNewer: boolean;
}

/** `Result<T, LoggerError>` error payload (CONTRACTS.md §2 + §7). */
export interface LoggerError {
  code:
    | 'source_not_found'
    | 'session_active'
    | 'no_such_roast'
    | 'db'
    | 'io'
    | 'port_busy'
    | 'port_error'
    | 'parse'
    | 'invalid_args'
    | 'preview_active'
    | (string & {});
  message: string;
}

export type { SourceInfo };
