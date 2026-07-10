//! ipc.rs — the Tauri command surface (CONTRACTS.md §2, frozen).
//!
//! Every command returns `Result<T, LoggerError>` (`{ code, message }`);
//! DTOs serialize camelCase; the sample stream flows on an ORDERED
//! `tauri::ipc::Channel<SampleEvent>` (never window events, which can reorder).

use std::sync::Arc;

use tauri::ipc::Channel;
use tauri::State;

use crate::capture::{self, Emitter, Engine};
use crate::error::LoggerError;
use crate::model::{
    AppInfoDto, CoffeeDto, ExportResultDto, ExportRoastArgs, FinishSessionArgs, GetRoastResult,
    GetSettingArgs, ImportPathsArgs, ImportPreviewDto, ImportResultDto, LocalIdArgs, MachineDto,
    MarkEventArgs, PreviewEvent, RecoveryDto, ResumeSessionResult, RoastMarkersDto, RoastRefArgs,
    RoastSummaryDto, SampleEvent, SaveCoffeeArgs, SaveMachineArgs, SerialPortDto, SetSettingArgs,
    SniffPortArgs, SniffResultDto, SourceInfo, StartPortPreviewArgs, StartSessionArgs,
    StartSessionResult, UndoEventArgs, UpdateInfoDto, UpdateRoastArgs, UpdateRoastMarkersArgs,
};

fn channel_emitter(on_event: Channel<SampleEvent>) -> Emitter {
    Box::new(move |ev| {
        if let Err(err) = on_event.send(ev) {
            tracing::debug!(error = %err, "channel send failed (webview gone?)");
        }
    })
}

#[tauri::command]
pub fn ping() -> &'static str {
    "pong"
}

/// Phase 1: the bundled replay fixtures. Device sources appear in Phase 2a.
#[tauri::command]
pub fn list_sources() -> Vec<SourceInfo> {
    capture::replay::list_replay_sources()
}

#[tauri::command]
pub async fn start_session(
    engine: State<'_, Arc<Engine>>,
    args: StartSessionArgs,
    on_event: Channel<SampleEvent>,
) -> Result<StartSessionResult, LoggerError> {
    engine
        .start_session(&args, channel_emitter(on_event))
        .map(|roast_uuid| StartSessionResult { roast_uuid })
}

#[tauri::command]
pub async fn mark_event(
    engine: State<'_, Arc<Engine>>,
    args: MarkEventArgs,
) -> Result<RoastMarkersDto, LoggerError> {
    engine
        .store()
        .mark_event(&args.roast_uuid, args.kind, args.session_sec, args.note)
}

#[tauri::command]
pub async fn undo_event(
    engine: State<'_, Arc<Engine>>,
    args: UndoEventArgs,
) -> Result<RoastMarkersDto, LoggerError> {
    engine.store().undo_event(&args.roast_uuid, args.kind)
}

#[tauri::command]
pub async fn finish_session(
    engine: State<'_, Arc<Engine>>,
    args: FinishSessionArgs,
) -> Result<RoastSummaryDto, LoggerError> {
    engine.stop_if_active(&args.roast_uuid);
    engine
        .store()
        .finish_roast(&args.roast_uuid, args.drop_weight_lb, args.notes)
}

#[tauri::command]
pub async fn abandon_session(
    engine: State<'_, Arc<Engine>>,
    args: RoastRefArgs,
) -> Result<(), LoggerError> {
    engine.stop_if_active(&args.roast_uuid);
    engine.store().abandon_roast(&args.roast_uuid)
}

/// Stop streaming samples WITHOUT finalizing the roast — the end of the
/// post-drop cool-down window. The roast stays `recording` until
/// finish_session / abandon_session (which are no-op-safe on a stopped source).
#[tauri::command]
pub async fn stop_capture(
    engine: State<'_, Arc<Engine>>,
    args: RoastRefArgs,
) -> Result<(), LoggerError> {
    engine.stop_if_active(&args.roast_uuid);
    Ok(())
}

#[tauri::command]
pub async fn pending_recovery(
    engine: State<'_, Arc<Engine>>,
) -> Result<Option<RecoveryDto>, LoggerError> {
    engine.store().pending_recovery(engine.active_roast_uuid())
}

#[tauri::command]
pub async fn resume_session(
    engine: State<'_, Arc<Engine>>,
    args: RoastRefArgs,
    on_event: Channel<SampleEvent>,
) -> Result<ResumeSessionResult, LoggerError> {
    engine
        .resume_session(&args.roast_uuid, channel_emitter(on_event))
        .map(|resumed_from_seq| ResumeSessionResult { resumed_from_seq })
}

#[tauri::command]
pub async fn discard_recovery(
    engine: State<'_, Arc<Engine>>,
    args: RoastRefArgs,
) -> Result<(), LoggerError> {
    if engine.active_roast_uuid().as_deref() == Some(args.roast_uuid.as_str()) {
        return Err(LoggerError::new(
            crate::error::ErrorCode::SessionActive,
            "cannot discard the active recording session; finish or abandon it instead",
        ));
    }
    engine.store().discard_recovery(&args.roast_uuid)
}

#[tauri::command]
pub async fn list_roasts(
    engine: State<'_, Arc<Engine>>,
) -> Result<Vec<RoastSummaryDto>, LoggerError> {
    engine.store().list_roasts()
}

#[tauri::command]
pub async fn get_roast(
    engine: State<'_, Arc<Engine>>,
    args: RoastRefArgs,
) -> Result<GetRoastResult, LoggerError> {
    engine.store().get_roast(&args.roast_uuid)
}

#[tauri::command]
pub async fn get_setting(
    engine: State<'_, Arc<Engine>>,
    args: GetSettingArgs,
) -> Result<Option<String>, LoggerError> {
    engine.store().get_setting(&args.key)
}

#[tauri::command]
pub async fn set_setting(
    engine: State<'_, Arc<Engine>>,
    args: SetSettingArgs,
) -> Result<(), LoggerError> {
    engine.store().set_setting(&args.key, &args.value)
}

// ---------------------------------------------------------------------------
// §7.1 import / export (module src/alog + store/imports.rs — owner: importer)
// ---------------------------------------------------------------------------

/// Parse-only preview; never writes.
#[tauri::command]
pub async fn preview_import(args: ImportPathsArgs) -> Result<Vec<ImportPreviewDto>, LoggerError> {
    crate::alog::preview(&args.paths)
}

/// Re-parse and commit the given files.
#[tauri::command]
pub async fn import_roasts(
    engine: State<'_, Arc<Engine>>,
    args: ImportPathsArgs,
) -> Result<ImportResultDto, LoggerError> {
    let device_id = engine
        .store()
        .get_setting("device_id")?
        .ok_or_else(|| LoggerError::db("device_id missing (minted on first run)"))?;
    crate::alog::import(engine.store(), &device_id, &args.paths)
}

/// `destPath` is chosen by the frontend via the dialog plugin.
#[tauri::command]
pub async fn export_roast(
    engine: State<'_, Arc<Engine>>,
    args: ExportRoastArgs,
) -> Result<ExportResultDto, LoggerError> {
    crate::alog::export(engine.store(), &args)
}

// ---------------------------------------------------------------------------
// §7.2 serial + TC4 (modules capture/serial.rs, capture/tc4.rs — owner: driver)
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn list_serial_ports() -> Result<Vec<SerialPortDto>, LoggerError> {
    capture::serial::list_ports()
}

#[tauri::command]
pub async fn sniff_port(args: SniffPortArgs) -> Result<SniffResultDto, LoggerError> {
    capture::serial::sniff(&args)
}

/// One preview at a time (`preview_active`, owned by serial.rs); refused while
/// a session is active (`session_active`, enforced here where the Engine lives).
#[tauri::command]
pub async fn start_port_preview(
    engine: State<'_, Arc<Engine>>,
    args: StartPortPreviewArgs,
    on_event: Channel<PreviewEvent>,
) -> Result<(), LoggerError> {
    if engine.active_roast_uuid().is_some() {
        return Err(LoggerError::session_active());
    }
    let emitter: capture::serial::PreviewEmitter = Box::new(move |ev| {
        if let Err(err) = on_event.send(ev) {
            tracing::debug!(error = %err, "preview channel send failed (webview gone?)");
        }
    });
    capture::serial::start_preview(&args, emitter)
}

#[tauri::command]
pub async fn stop_port_preview() -> Result<(), LoggerError> {
    capture::serial::stop_preview()
}

// ---------------------------------------------------------------------------
// §7.3 library (module store/library.rs — owner: librarian)
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn list_machines(engine: State<'_, Arc<Engine>>) -> Result<Vec<MachineDto>, LoggerError> {
    engine.store().list_machines()
}

#[tauri::command]
pub async fn save_machine(
    engine: State<'_, Arc<Engine>>,
    args: SaveMachineArgs,
) -> Result<MachineDto, LoggerError> {
    engine.store().save_machine(&args)
}

#[tauri::command]
pub async fn archive_machine(
    engine: State<'_, Arc<Engine>>,
    args: LocalIdArgs,
) -> Result<(), LoggerError> {
    engine.store().archive_machine(args.id)
}

#[tauri::command]
pub async fn list_coffees(engine: State<'_, Arc<Engine>>) -> Result<Vec<CoffeeDto>, LoggerError> {
    engine.store().list_coffees()
}

#[tauri::command]
pub async fn save_coffee(
    engine: State<'_, Arc<Engine>>,
    args: SaveCoffeeArgs,
) -> Result<CoffeeDto, LoggerError> {
    engine.store().save_coffee(&args)
}

#[tauri::command]
pub async fn archive_coffee(
    engine: State<'_, Arc<Engine>>,
    args: LocalIdArgs,
) -> Result<(), LoggerError> {
    engine.store().archive_coffee(args.id)
}

#[tauri::command]
pub async fn update_roast(
    engine: State<'_, Arc<Engine>>,
    args: UpdateRoastArgs,
) -> Result<RoastSummaryDto, LoggerError> {
    engine.store().update_roast(&args.roast_uuid, &args.patch)
}

#[tauri::command]
pub async fn update_roast_markers(
    engine: State<'_, Arc<Engine>>,
    args: UpdateRoastMarkersArgs,
) -> Result<RoastMarkersDto, LoggerError> {
    engine
        .store()
        .update_roast_markers(&args.roast_uuid, &args.markers)
}

/// Hard-deletes roast + samples + events + outbox rows; refused for the
/// active session (CONTRACTS §7.3).
#[tauri::command]
pub async fn delete_roast(
    engine: State<'_, Arc<Engine>>,
    args: RoastRefArgs,
) -> Result<(), LoggerError> {
    if engine.active_roast_uuid().as_deref() == Some(args.roast_uuid.as_str()) {
        return Err(LoggerError::new(
            crate::error::ErrorCode::SessionActive,
            "cannot delete the active recording session; finish or abandon it first",
        ));
    }
    engine.store().delete_roast(&args.roast_uuid)
}

// ---------------------------------------------------------------------------
// §7.4 app shell (modules menu.rs, update.rs — owner: polisher)
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_app_info(app: tauri::AppHandle) -> Result<AppInfoDto, LoggerError> {
    Ok(crate::update::app_info(&app))
}

/// Notice-only in v0.1.0; errors map to `Ok(None)`, never a user-facing failure.
#[tauri::command]
pub async fn check_for_update(app: tauri::AppHandle) -> Result<Option<UpdateInfoDto>, LoggerError> {
    let current = crate::update::app_info(&app).version;
    crate::update::check_for_update(&current)
}
