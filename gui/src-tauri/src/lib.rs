use crate::commands::add_handlers;
use anyhow::Context;
use savegame_timeline_core::detection;
use savegame_timeline_core::{OpenOptions, TimelineCore};
use std::sync::Arc;
use tauri::{App, Emitter, Manager};

pub mod commands;
mod tray;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .setup(init);

    let builder = add_handlers(builder);

    builder
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // Capture pending changes before the process ends.
            if let tauri::RunEvent::Exit = event {
                if let Some(core) = app.try_state::<Arc<TimelineCore>>() {
                    core.shutdown();
                }
            }
        });
}

fn init(app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = app
        .path()
        .app_local_data_dir()
        .context("failed to get app data directory")?;
    let (core, notifications) = TimelineCore::open(
        &data_dir,
        OpenOptions {
            watch: true,
            debounce_seconds: None,
        },
    )?;
    let core = Arc::new(core);
    app.manage(core.clone());
    tray::setup_tray(app)?;

    // Sync (when configured) and automatic pruning, every five minutes.
    let background_app = app.handle().clone();
    let background_core = core.clone();
    std::thread::spawn(move || loop {
        match background_core.sync_if_configured() {
            Ok(Some(report)) => {
                background_app.emit("sync-complete", report).ok();
            }
            Ok(None) => {}
            Err(error) => {
                background_app.emit("sync-error", format!("{error:#}")).ok();
            }
        }
        match background_core.prune_if_due() {
            Ok(Some(report)) if report.changed_history() => {
                background_app.emit("history-pruned", ()).ok();
            }
            Ok(_) => {}
            Err(error) => {
                background_app
                    .emit("pruning-error", format!("{error:#}"))
                    .ok();
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(5 * 60));
    });

    let notif_app = app.handle().clone();
    let checkpoint_core = core.clone();
    std::thread::spawn(move || {
        for notification in notifications {
            match notification {
                detection::Notification::ChangeDetected { game_id } => {
                    notif_app.emit("change-detected", game_id).ok();
                }
                detection::Notification::VersionCreated {
                    game_id,
                    version_number,
                } => {
                    notif_app
                        .emit("version-created", (game_id, version_number))
                        .ok();
                    if let Err(error) = checkpoint_core.push_sync() {
                        notif_app.emit("sync-error", format!("{error:#}")).ok();
                    }
                }
                detection::Notification::Error(err) => {
                    notif_app.emit("detection-error", err).ok();
                }
            }
        }
    });

    Ok(())
}
