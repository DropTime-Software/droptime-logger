//! menu.rs — the native application menu (CONTRACTS.md §7.4).
//!
//! Owner: polisher (extends this). Integration ships the minimal real menu:
//! App (About, Check for Updates…, Settings… ⌘,), standard Edit/Window
//! predefined items so copy/paste works, and Help (Keyboard Shortcuts,
//! Report an Issue). Selecting one of OUR items emits the webview event
//! `menu://<id>` (ids: `about`, `check-updates`, `settings`, `shortcuts`,
//! `report-issue`), handled in `features/appShell` on the TS side.

use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};
use tauri::{App, Emitter};

/// Ids we emit to the webview as `menu://<id>`.
const EMITTED_IDS: [&str; 5] = [
    "about",
    "check-updates",
    "settings",
    "shortcuts",
    "report-issue",
];

pub fn install(app: &App) -> tauri::Result<()> {
    let about = MenuItemBuilder::with_id("about", "About Droptime Logger").build(app)?;
    let check_updates =
        MenuItemBuilder::with_id("check-updates", "Check for Updates…").build(app)?;
    let settings = MenuItemBuilder::with_id("settings", "Settings…")
        .accelerator("CmdOrCtrl+,")
        .build(app)?;
    let shortcuts = MenuItemBuilder::with_id("shortcuts", "Keyboard Shortcuts").build(app)?;
    let report_issue = MenuItemBuilder::with_id("report-issue", "Report an Issue").build(app)?;

    // On macOS the first submenu becomes the application menu.
    let app_menu = SubmenuBuilder::new(app, "Droptime Logger")
        .item(&about)
        .item(&PredefinedMenuItem::separator(app)?)
        .item(&check_updates)
        .item(&settings)
        .item(&PredefinedMenuItem::separator(app)?)
        .item(&PredefinedMenuItem::hide(app, None)?)
        .item(&PredefinedMenuItem::hide_others(app, None)?)
        .item(&PredefinedMenuItem::show_all(app, None)?)
        .item(&PredefinedMenuItem::separator(app)?)
        .item(&PredefinedMenuItem::quit(app, None)?)
        .build()?;

    // Standard Edit items so copy/paste/select-all work in the webview.
    let edit_menu = SubmenuBuilder::new(app, "Edit")
        .item(&PredefinedMenuItem::undo(app, None)?)
        .item(&PredefinedMenuItem::redo(app, None)?)
        .item(&PredefinedMenuItem::separator(app)?)
        .item(&PredefinedMenuItem::cut(app, None)?)
        .item(&PredefinedMenuItem::copy(app, None)?)
        .item(&PredefinedMenuItem::paste(app, None)?)
        .item(&PredefinedMenuItem::select_all(app, None)?)
        .build()?;

    let window_menu = SubmenuBuilder::new(app, "Window")
        .item(&PredefinedMenuItem::minimize(app, None)?)
        .item(&PredefinedMenuItem::maximize(app, None)?)
        .item(&PredefinedMenuItem::separator(app)?)
        .item(&PredefinedMenuItem::close_window(app, None)?)
        .build()?;

    let help_menu = SubmenuBuilder::new(app, "Help")
        .item(&shortcuts)
        .item(&report_issue)
        .build()?;

    let menu = MenuBuilder::new(app)
        .item(&app_menu)
        .item(&edit_menu)
        .item(&window_menu)
        .item(&help_menu)
        .build()?;
    app.set_menu(menu)?;

    app.on_menu_event(|handle, event| {
        let id = event.id().as_ref();
        if EMITTED_IDS.contains(&id) {
            if let Err(err) = handle.emit(&format!("menu://{id}"), ()) {
                tracing::warn!(error = %err, id, "menu event emit failed");
            }
        }
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::EMITTED_IDS;

    #[test]
    fn emits_exactly_the_ids_the_webview_handles() {
        // The `features/appShell` menu-event listeners key off these literals
        // (`menu://<id>`); keep the two sides in lock-step.
        assert_eq!(
            EMITTED_IDS,
            [
                "about",
                "check-updates",
                "settings",
                "shortcuts",
                "report-issue"
            ]
        );
    }
}
