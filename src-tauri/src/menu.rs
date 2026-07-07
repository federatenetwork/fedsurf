//! Native menu: item ids double as `tabs::dispatch_action` action names, so
//! accelerators (⌘T/⌘W/⌃Tab/…) work even while a page webview has focus —
//! on macOS this is the only reliable path, and it also provides the standard
//! Edit bindings (⌘C/⌘V/…) that WKWebView requires a menu for.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::App;

pub fn install(app: &App) -> tauri::Result<()> {
    let handle = app.handle();

    let item = |id: &str, label: &str, accel: &str| {
        MenuItem::with_id(handle, id, label, true, Some(accel))
    };

    #[cfg(target_os = "macos")]
    let app_menu = Submenu::with_items(
        handle,
        "Fedsurf",
        true,
        &[
            &PredefinedMenuItem::about(handle, None, None)?,
            &PredefinedMenuItem::separator(handle)?,
            &PredefinedMenuItem::hide(handle, None)?,
            &PredefinedMenuItem::hide_others(handle, None)?,
            &PredefinedMenuItem::show_all(handle, None)?,
            &PredefinedMenuItem::separator(handle)?,
            &PredefinedMenuItem::quit(handle, None)?,
        ],
    )?;

    let file_menu = Submenu::with_items(
        handle,
        "File",
        true,
        &[
            &item("new-tab", "New Tab", "CmdOrCtrl+T")?,
            &item("close-tab", "Close Tab", "CmdOrCtrl+W")?,
        ],
    )?;

    let edit_menu = Submenu::with_items(
        handle,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(handle, None)?,
            &PredefinedMenuItem::redo(handle, None)?,
            &PredefinedMenuItem::separator(handle)?,
            &PredefinedMenuItem::cut(handle, None)?,
            &PredefinedMenuItem::copy(handle, None)?,
            &PredefinedMenuItem::paste(handle, None)?,
            &PredefinedMenuItem::select_all(handle, None)?,
        ],
    )?;

    let view_menu = Submenu::with_items(
        handle,
        "View",
        true,
        &[
            &item("reload", "Reload Page", "CmdOrCtrl+R")?,
            &PredefinedMenuItem::separator(handle)?,
            &item("back", "Back", "CmdOrCtrl+[")?,
            &item("forward", "Forward", "CmdOrCtrl+]")?,
            &PredefinedMenuItem::separator(handle)?,
            &item("toggle-sidebar", "Toggle Sidebar", "CmdOrCtrl+B")?,
            &item("focus-address", "Open Location", "CmdOrCtrl+L")?,
        ],
    )?;

    let tab_menu = Submenu::with_items(
        handle,
        "Tabs",
        true,
        &[
            &item("next-tab", "Show Next Tab", "Control+Tab")?,
            &item("prev-tab", "Show Previous Tab", "Control+Shift+Tab")?,
        ],
    )?;

    #[cfg(target_os = "macos")]
    let window_menu = Submenu::with_items(
        handle,
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(handle, None)?,
            &PredefinedMenuItem::maximize(handle, None)?,
        ],
    )?;

    let menu = Menu::with_items(
        handle,
        &[
            #[cfg(target_os = "macos")]
            &app_menu,
            &file_menu,
            &edit_menu,
            &view_menu,
            &tab_menu,
            #[cfg(target_os = "macos")]
            &window_menu,
        ],
    )?;
    app.set_menu(menu)?;
    app.on_menu_event(|app, event| {
        crate::tabs::dispatch_action(app, event.id().as_ref());
    });
    Ok(())
}
