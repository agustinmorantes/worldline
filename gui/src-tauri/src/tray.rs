use serde::Deserialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{
    menu::{MenuBuilder, MenuItem, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, WindowEvent,
};
use tauri_plugin_notification::NotificationExt;

/// The tray's text, in the language the GUI shows (ADR 0007).
#[derive(Debug, Clone, Deserialize)]
pub struct TrayLabels {
    pub open: String,
    pub exit: String,
    pub tooltip: String,
    pub hidden_hint_title: Option<String>,
    pub hidden_hint_body: String,
}

impl Default for TrayLabels {
    /// Shown until the GUI sends its own.
    fn default() -> Self {
        Self {
            open: "Open".into(),
            exit: "Exit".into(),
            tooltip: "Worldline".into(),
            hidden_hint_title: None,
            hidden_hint_body: "Still running in the tray. Right-click the icon to exit.".into(),
        }
    }
}

pub struct TrayState {
    pub exit_requested: AtomicBool,
    pub has_shown_hint: AtomicBool,
    labels: Mutex<TrayLabels>,
    open_item: MenuItem<tauri::Wry>,
    exit_item: MenuItem<tauri::Wry>,
    tray: TrayIcon,
}

pub fn setup_tray(app: &tauri::App) -> anyhow::Result<()> {
    let labels = TrayLabels::default();
    let open_item = MenuItemBuilder::with_id("open", &labels.open).build(app)?;
    let exit_item = MenuItemBuilder::with_id("exit", &labels.exit).build(app)?;
    let menu = MenuBuilder::new(app)
        .items(&[&open_item, &exit_item])
        .build()?;

    let icon = app
        .default_window_icon()
        .ok_or_else(|| anyhow::anyhow!("no default window icon"))?
        .clone();

    let tray = TrayIconBuilder::new()
        .icon(icon)
        .tooltip(&labels.tooltip)
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

    app.manage(TrayState {
        exit_requested: AtomicBool::new(false),
        has_shown_hint: AtomicBool::new(false),
        labels: Mutex::new(labels),
        open_item,
        exit_item,
        tray,
    });

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
                let labels = state
                    .labels
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                let _ = win
                    .app_handle()
                    .notification()
                    .builder()
                    .title(labels.hidden_hint_title.as_deref().unwrap_or("Worldline"))
                    .body(labels.hidden_hint_body)
                    .show();
            }
        }
    });

    Ok(())
}

/// Replaces the tray's text, updating the live menu and tooltip.
pub fn set_labels(app: &AppHandle, labels: TrayLabels) -> tauri::Result<()> {
    let state = app.state::<TrayState>();
    state.open_item.set_text(&labels.open)?;
    state.exit_item.set_text(&labels.exit)?;
    state.tray.set_tooltip(Some(&labels.tooltip))?;
    *state.labels.lock().unwrap_or_else(|e| e.into_inner()) = labels;
    Ok(())
}

fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}
