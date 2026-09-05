use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, WindowEvent,
};
use tauri_plugin_notification::NotificationExt;

pub struct TrayState {
    pub exit_requested: AtomicBool,
    pub has_shown_hint: AtomicBool,
}

pub fn setup_tray(app: &tauri::App) -> anyhow::Result<()> {
    app.manage(TrayState {
        exit_requested: AtomicBool::new(false),
        has_shown_hint: AtomicBool::new(false),
    });

    let open_item = MenuItemBuilder::with_id("open", "Open").build(app)?;
    let exit_item = MenuItemBuilder::with_id("exit", "Exit").build(app)?;
    let menu = MenuBuilder::new(app)
        .items(&[&open_item, &exit_item])
        .build()?;

    let icon = app
        .default_window_icon()
        .ok_or_else(|| anyhow::anyhow!("no default window icon"))?
        .clone();

    TrayIconBuilder::new()
        .icon(icon)
        .tooltip("savegame-timeline")
        .menu(&menu)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_window(tray.app_handle());
            }
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_window(app),
            "exit" => {
                app.state::<TrayState>()
                    .exit_requested
                    .store(true, Ordering::Relaxed);
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;

    let window = app
        .get_webview_window("main")
        .ok_or_else(|| anyhow::anyhow!("main window not found"))?;

    let win = window.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            let state = win.state::<TrayState>();
            if state.exit_requested.load(Ordering::Relaxed) {
                return;
            }
            api.prevent_close();
            let _ = win.hide();
            if !state.has_shown_hint.swap(true, Ordering::Relaxed) {
                let _ = win
                    .app_handle()
                    .notification()
                    .builder()
                    .title("savegame-timeline")
                    .body("Still running in the tray. Right-click the icon to exit.")
                    .show();
            }
        }
    });

    Ok(())
}

fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}
