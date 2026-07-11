// Droptime Logger — Rust core.
//
// Architecture rules (binding, from docs/build-plans/droptime-logger.md §3):
//   1. Capture + persistence live HERE. The webview only renders — its timers
//      throttle when minimized on Windows/Linux and must never gate the roast
//      log.
//   2. Samples reach SQLite before the UI: capture tick = read source → append
//      to `samples` (batched per-second transaction) → emit on the IPC Channel.
//
// Modules (Phase 1):
//   capture/ — engine + DeviceSource trait + replay driver (driver #0)
//   store/   — rusqlite layer: schema (user_version ladder), single writer
//              thread, batched sample appends, outbox
//   ipc.rs   — #[tauri::command] surface + Channel<SampleEvent> streaming
//   model.rs — DTOs mirroring packages/roast-console/src/types.ts
//   math.rs  — numeric parity with packages/roast-console/src/math
//   See ../../CONTRACTS.md for the frozen IPC + DB contract.

mod alog;
mod capture;
mod cloud;
mod error;
mod ipc;
mod math;
mod menu;
mod model;
mod store;
mod update;

use std::sync::Arc;

use tauri::Manager;

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "droptime_logger_lib=debug,info".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // DB file per CONTRACTS.md §1: {app_local_data_dir}/droptime-logger.db
            let dir = app.path().app_local_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let db_path = dir.join("droptime-logger.db");
            tracing::info!(db = %db_path.display(), "opening roast store");
            let store = store::Store::open(&db_path)?;
            app.manage(Arc::new(capture::Engine::new(store)));
            menu::install(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ipc::ping,
            ipc::list_sources,
            ipc::start_session,
            ipc::mark_event,
            ipc::undo_event,
            ipc::finish_session,
            ipc::abandon_session,
            ipc::stop_capture,
            ipc::pending_recovery,
            ipc::resume_session,
            ipc::discard_recovery,
            ipc::list_roasts,
            ipc::get_roast,
            ipc::get_setting,
            ipc::set_setting,
            // v0.1.0 expansion (CONTRACTS.md §7)
            ipc::preview_import,
            ipc::import_roasts,
            ipc::export_roast,
            ipc::list_serial_ports,
            ipc::sniff_port,
            ipc::start_port_preview,
            ipc::stop_port_preview,
            ipc::list_machines,
            ipc::save_machine,
            ipc::archive_machine,
            ipc::list_coffees,
            ipc::save_coffee,
            ipc::archive_coffee,
            ipc::update_roast,
            ipc::update_roast_markers,
            ipc::delete_roast,
            ipc::get_app_info,
            ipc::check_for_update,
            // Cloud sync (Droptime Cloud — store/sync.rs, cloud.rs)
            ipc::sync_pending,
            ipc::sync_pending_count,
            ipc::sync_mark_synced,
            ipc::oauth_start,
            ipc::cloud_fetch,
            ipc::cloud_clear_session,
        ])
        .build(tauri::generate_context!())
        .expect("error while building droptime-logger")
        .run(|app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                // Stop capture cleanly but leave any recording roast untouched:
                // pending_recovery picks it up on next launch (plan §5).
                if let Some(engine) = app_handle.try_state::<Arc<capture::Engine>>() {
                    engine.shutdown();
                }
            }
        });
}
