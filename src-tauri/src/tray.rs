use crate::types::TrackerState;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

/// Reload the ERP page in the `main` webview only (never the tracker overlay).
pub fn reload_erp(app: &AppHandle, hard: bool) {
    let Some(w) = app.get_webview_window("main") else {
        return;
    };
    if hard {
        match w.url() {
            Ok(url) => {
                let _ = w.navigate(url);
            }
            Err(_) => {
                let _ = w.eval("location.reload(true)");
            }
        }
    } else {
        let _ = w.reload();
    }
}

fn on_reload_menu_id(app: &AppHandle, id: &str) {
    match id {
        "reload_erp" | "view_reload_erp" | "view_reload_erp_f5" => reload_erp(app, false),
        "view_hard_reload_erp" => reload_erp(app, true),
        _ => {}
    }
}

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let open_erp = MenuItem::with_id(app, "open_erp", "Open ERP", true, None::<&str>)?;
    let reload_erp_item = MenuItem::with_id(app, "reload_erp", "Reload ERP", true, None::<&str>)?;
    let open_tracker = MenuItem::with_id(app, "open_tracker", "Open tracker", true, None::<&str>)?;
    let status = MenuItem::with_id(app, "status", "Signed out", false, None::<&str>)?;
    let idle = MenuItem::with_id(app, "idle", "Not checked in", false, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &open_erp,
            &reload_erp_item,
            &open_tracker,
            &sep,
            &status,
            &idle,
            &sep,
            &quit,
        ],
    )?;

    TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open_erp" => show_window(app, "main"),
            "reload_erp" => reload_erp(app, false),
            "open_tracker" => show_window(app, "tracker"),
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;

    // App menu accelerators: Cmd+R / F5 / Cmd+Shift+R. Always reloads `main`, not tracker.
    let reload = MenuItem::with_id(app, "view_reload_erp", "Reload", true, Some("CmdOrCtrl+R"))?;
    let reload_f5 = MenuItem::with_id(app, "view_reload_erp_f5", "Reload", true, Some("F5"))?;
    let hard_reload = MenuItem::with_id(
        app,
        "view_hard_reload_erp",
        "Hard Reload",
        true,
        Some("CmdOrCtrl+Shift+R"),
    )?;
    let view = Submenu::with_items(app, "View", true, &[&reload, &reload_f5, &hard_reload])?;
    let app_menu = Menu::default(app)?;
    app_menu.append(&view)?;
    app.set_menu(app_menu)?;
    app.on_menu_event(|app, event| {
        on_reload_menu_id(app, event.id().as_ref());
    });

    Ok(())
}

fn show_window(app: &AppHandle, label: &str) {
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

pub fn rebuild(_app: &AppHandle, _state: &TrackerState) {}
