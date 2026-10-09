use crate::commands::add_handlers;
use anyhow::Context;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{App, Emitter, Manager};
use worldline_core::detection;
use worldline_core::{OpenOptions, WorldlineCore};

pub mod commands;
mod dialogs;
mod tray;

/// Where the app keeps its data: the index, history, icons and GUI preferences.
pub struct DataDir(pub PathBuf);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(tauri_plugin_log::log::LevelFilter::Info)
                .build(),
        )
        .setup(init);

    let builder = add_handlers(builder);

    builder
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // Capture pending changes before the process ends.
            if let tauri::RunEvent::Exit = event {
                if let Some(core) = app.try_state::<Arc<WorldlineCore>>() {
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
    #[cfg(feature = "e2e")]
    let data_dir = std::env::var_os("WORLDLINE_E2E_DATA_DIR").map_or(data_dir, PathBuf::from);
    // The asset scope in `tauri.conf.json` names the real data directory.
    #[cfg(feature = "e2e")]
    app.asset_protocol_scope()
        .allow_directory(data_dir.join("repository"), true)?;
    app.manage(DataDir(data_dir.clone()));
    let (core, notifications) = WorldlineCore::open_shared(
        &data_dir,
        OpenOptions {
            watch: true,
            debounce_seconds: None,
        },
    )?;
    app.manage(core);
    tray::setup_tray(app)?;

    let notif_app = app.handle().clone();
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
                }
                detection::Notification::ScreenshotAttached {
                    game_id,
                    version_id,
                } => {
                    notif_app
                        .emit("screenshot-attached", (game_id, version_id))
                        .ok();
                }
                detection::Notification::Error(err) => {
                    notif_app.emit("detection-error", err).ok();
                }
                detection::Notification::SyncStarted => {
                    notif_app.emit("sync-started", ()).ok();
                }
                detection::Notification::SyncCompleted(report) => {
                    notif_app.emit("sync-complete", report).ok();
                }
                detection::Notification::SyncFailed(error) => {
                    notif_app.emit("sync-error", error).ok();
                }
                detection::Notification::HistoryPruned => {
                    notif_app.emit("history-pruned", ()).ok();
                }
                detection::Notification::PruningFailed(error) => {
                    notif_app.emit("pruning-error", error).ok();
                }
                detection::Notification::GameStarted { game_id } => {
                    notif_app.emit("game-started", game_id).ok();
                }
                detection::Notification::GameStopped { game_id } => {
                    notif_app.emit("game-stopped", game_id).ok();
                }
            }
        }
    });

    Ok(())
}
