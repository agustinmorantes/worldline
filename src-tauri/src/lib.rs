use crate::commands::add_handlers;
use anyhow::Context;
use diesel::r2d2::{ConnectionManager, Pool};
use diesel::SqliteConnection;
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
use std::sync::Arc;
use tauri::{App, Emitter, Manager};

pub mod commands;
pub mod models;
pub mod schema;
pub mod detection;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
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

    let db_path = data_dir.join("database.db");
    println!("db path: {}", db_path.display());

    let database_url = format!("file:{}?foreign_keys=on", db_path.display());
    let manager = ConnectionManager::<SqliteConnection>::new(&database_url);
    let pool = Pool::builder()
        .build(manager)
        .context("Failed to create pool.")?;
    let db = Arc::new(pool);

    db.get()?
        .run_pending_migrations(MIGRATIONS)
        .expect("Error running migrations");

    app.manage(db);

    let app_handle = app.handle();
    let detection_app = app_handle.clone();
    let (cmd_tx, notif_rx) =  detection::init(detection_app)?;
    app.manage(cmd_tx);

    // Handle notifications from detection system
    let notif_app = app_handle.clone();
    std::thread::spawn(move || {
        for notification in notif_rx {
            match notification {
                detection::Notification::ChangeDetected { game_id } => {
                    notif_app.emit("change-detected", game_id)
                        .ok();
                }
                detection::Notification::VersionCreated { game_id, version_id } => {
                    notif_app.emit("version-created", (game_id, version_id))
                        .ok();
                }
                detection::Notification::Error(err) => {
                    notif_app.emit("detection-error", err)
                        .ok();
                }
            }
        }
    });

    Ok(())
}

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!();

type DbPool = Arc<Pool<ConnectionManager<SqliteConnection>>>;
