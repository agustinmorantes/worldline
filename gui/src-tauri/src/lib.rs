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
    app.manage(Arc::new(core));
    tray::setup_tray(app)?;

    let notif_app = app.handle().clone();
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
                }
                detection::Notification::Error(err) => {
                    notif_app.emit("detection-error", err).ok();
                }
            }
        }
    });

    Ok(())
}
