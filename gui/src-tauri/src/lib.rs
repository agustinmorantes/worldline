use crate::commands::add_handlers;
use anyhow::Context;
use diesel::r2d2::{ConnectionManager, Pool};
use diesel::SqliteConnection;
use diesel_migrations::MigrationHarness;
use savegame_timeline_core::detection;
use savegame_timeline_core::persistence::sqlite::{
    SqliteGameRepository, SqliteScanRootsRepository, SqliteVersionRepository,
};
use savegame_timeline_core::TimelineCore;
use savegame_timeline_core::MIGRATIONS;
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
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn init(app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = app
        .path()
        .app_local_data_dir()
        .context("failed to get app data directory")?;
    std::fs::create_dir_all(&data_dir).context("failed to create data directory ")?;

    let working_base = data_dir.join("repository");
    let icons_dir = data_dir.join("icons");

    let db_path = data_dir.join("database.db");
    println!("db path: {}", db_path.display());

    let database_url = format!("file:{}?foreign_keys=on", db_path.display());
    let manager = ConnectionManager::<SqliteConnection>::new(&database_url);
    let pool = Pool::builder()
        .build(manager)
        .context("Failed to create pool.")?;

    let mut conn = pool.get()?;
    conn.run_pending_migrations(MIGRATIONS)
        .expect("Error running migrations");

    let games_repo = Arc::new(SqliteGameRepository::with_repository_root(
        pool.clone(),
        working_base.clone(),
    ));
    let versions_repo = Arc::new(SqliteVersionRepository::with_repository_root(
        pool.clone(),
        working_base.clone(),
    ));
    let scan_roots_repo = Arc::new(SqliteScanRootsRepository::new(pool.clone()));

    let (core, notif_rx) = TimelineCore::new(
        games_repo,
        versions_repo,
        scan_roots_repo,
        working_base,
        icons_dir,
        None,
    )?;
    let core = Arc::new(core);
    app.manage(core.clone());
    tray::setup_tray(app)?;

    let sync_app = app.handle().clone();
    let periodic_core = core.clone();
    std::thread::spawn(move || loop {
        if periodic_core
            .get_sync_settings()
            .is_ok_and(|settings| settings.remote_path.is_some())
        {
            match periodic_core.refresh_sync() {
                Ok(report) => {
                    sync_app.emit("sync-complete", report).ok();
                }
                Err(error) => {
                    sync_app.emit("sync-error", error.to_string()).ok();
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(5 * 60));
    });

    let pruning_app = app.handle().clone();
    let pruning_core = core.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(60));
        loop {
            match pruning_core.get_pruning_settings() {
                Ok(settings) if settings.automatic => match pruning_core.run_pruning() {
                    Ok(report) => {
                        if !report.retired_versions.is_empty()
                            || report.local_gc.reclaimable_blobs > 0
                            || report
                                .remote_gc
                                .as_ref()
                                .is_some_and(|gc| gc.reclaimable_blobs > 0)
                        {
                            pruning_app.emit("history-pruned", ()).ok();
                        }
                    }
                    Err(error) => {
                        pruning_app.emit("pruning-error", error.to_string()).ok();
                    }
                },
                Err(error) => {
                    pruning_app.emit("pruning-error", error.to_string()).ok();
                }
                _ => {}
            }
            std::thread::sleep(std::time::Duration::from_secs(4 * 60 * 60));
        }
    });

    let notif_app = app.handle().clone();
    let checkpoint_core = core.clone();
    std::thread::spawn(move || {
        for notification in notif_rx {
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
                    if checkpoint_core
                        .get_sync_settings()
                        .is_ok_and(|settings| settings.remote_path.is_some())
                    {
                        if let Err(error) = checkpoint_core.push_sync() {
                            notif_app.emit("sync-error", error.to_string()).ok();
                        }
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
