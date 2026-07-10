/**
 * Typed wrappers over the Tauri command surface (CONTRACTS.md §2).
 *
 * Every command is `Result<T, LoggerError>` on the Rust side; `invoke` rejects with
 * the serialized `LoggerError` (`{ code, message }`) which callers may narrow via
 * `asLoggerError`. Commands that carry a live stream take a `Channel<SampleEvent>`
 * as the `onEvent` argument.
 */
import { invoke, type Channel } from '@tauri-apps/api/core';
import type {
  AppInfo,
  CoffeeDto,
  ExportRoastArgs,
  FinishSessionArgs,
  GetRoastResult,
  ImportPreviewDto,
  ImportResultDto,
  LoggerError,
  MachineDto,
  MarkEventArgs,
  MarkerPatch,
  PreviewEvent,
  RecoveryDto,
  ResumeSessionResult,
  RoastMarkersDto,
  RoastPatch,
  RoastSummaryDto,
  SampleEvent,
  SaveCoffeeArgs,
  SaveMachineArgs,
  SerialPortDto,
  SniffPortArgs,
  SniffResultDto,
  SourceInfo,
  StartPortPreviewArgs,
  StartSessionArgs,
  StartSessionResult,
  UndoEventArgs,
  UpdateInfo,
} from './dto';

export const ipc = {
  ping: () => invoke<string>('ping'),

  listSources: () => invoke<SourceInfo[]>('list_sources'),

  startSession: (args: StartSessionArgs, onEvent: Channel<SampleEvent>) =>
    invoke<StartSessionResult>('start_session', { args, onEvent }),

  markEvent: (args: MarkEventArgs) => invoke<RoastMarkersDto>('mark_event', { args }),

  undoEvent: (args: UndoEventArgs) => invoke<RoastMarkersDto>('undo_event', { args }),

  finishSession: (args: FinishSessionArgs) =>
    invoke<RoastSummaryDto>('finish_session', { args }),

  abandonSession: (args: { roastUuid: string }) =>
    invoke<void>('abandon_session', { args }),

  /** Stop streaming without finalizing — end of the post-drop cool-down. */
  stopCapture: (args: { roastUuid: string }) => invoke<void>('stop_capture', { args }),

  pendingRecovery: () => invoke<RecoveryDto | null>('pending_recovery'),

  resumeSession: (args: { roastUuid: string }, onEvent: Channel<SampleEvent>) =>
    invoke<ResumeSessionResult>('resume_session', { args, onEvent }),

  discardRecovery: (args: { roastUuid: string }) =>
    invoke<void>('discard_recovery', { args }),

  listRoasts: () => invoke<RoastSummaryDto[]>('list_roasts'),

  getRoast: (args: { roastUuid: string }) =>
    invoke<GetRoastResult>('get_roast', { args }),

  getSetting: (args: { key: string }) => invoke<string | null>('get_setting', { args }),

  setSetting: (args: { key: string; value: string }) =>
    invoke<void>('set_setting', { args }),

  // ---- §7.1 import / export ----

  /** Parse-only preview; never writes. */
  previewImport: (args: { paths: string[] }) =>
    invoke<ImportPreviewDto[]>('preview_import', { args }),

  importRoasts: (args: { paths: string[] }) =>
    invoke<ImportResultDto>('import_roasts', { args }),

  /** `destPath` chosen by the frontend via the dialog plugin. */
  exportRoast: (args: ExportRoastArgs) =>
    invoke<{ path: string }>('export_roast', { args }),

  // ---- §7.2 serial + TC4 ----

  listSerialPorts: () => invoke<SerialPortDto[]>('list_serial_ports'),

  sniffPort: (args: SniffPortArgs) => invoke<SniffResultDto>('sniff_port', { args }),

  /** One preview at a time; refused while a session is active. */
  startPortPreview: (args: StartPortPreviewArgs, onEvent: Channel<PreviewEvent>) =>
    invoke<void>('start_port_preview', { args, onEvent }),

  stopPortPreview: () => invoke<void>('stop_port_preview'),

  // ---- §7.3 library ----

  listMachines: () => invoke<MachineDto[]>('list_machines'),

  saveMachine: (args: SaveMachineArgs) => invoke<MachineDto>('save_machine', { args }),

  archiveMachine: (args: { id: number }) => invoke<void>('archive_machine', { args }),

  listCoffees: () => invoke<CoffeeDto[]>('list_coffees'),

  saveCoffee: (args: SaveCoffeeArgs) => invoke<CoffeeDto>('save_coffee', { args }),

  archiveCoffee: (args: { id: number }) => invoke<void>('archive_coffee', { args }),

  /** Patch semantics: absent = unchanged, explicit `null` = clear. */
  updateRoast: (args: { roastUuid: string; patch: RoastPatch }) =>
    invoke<RoastSummaryDto>('update_roast', { args }),

  /** Only on finished roasts; charge not editable in v0.1.0. */
  updateRoastMarkers: (args: { roastUuid: string; markers: MarkerPatch }) =>
    invoke<RoastMarkersDto>('update_roast_markers', { args }),

  /** Hard delete; refused for the active session. */
  deleteRoast: (args: { roastUuid: string }) => invoke<void>('delete_roast', { args }),

  // ---- §7.4 app shell ----

  getAppInfo: () => invoke<AppInfo>('get_app_info'),

  /** Notice-only; backend maps every failure to `null`. */
  checkForUpdate: () => invoke<UpdateInfo | null>('check_for_update'),
} as const;

/** Best-effort narrowing of a rejected invoke into a LoggerError. */
export function asLoggerError(err: unknown): LoggerError {
  if (err && typeof err === 'object' && 'code' in err && 'message' in err) {
    const e = err as { code: unknown; message: unknown };
    if (typeof e.code === 'string' && typeof e.message === 'string') {
      return { code: e.code, message: e.message };
    }
  }
  return { code: 'io', message: err instanceof Error ? err.message : String(err) };
}
