use anyhow::Context;
use diesel::r2d2::{ConnectionManager,Pool};
use diesel::SqliteConnection;
use std::sync::Arc;
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
use tauri::{App, Manager};
use crate::commands::add_handlers;

pub mod models;
pub mod schema;
pub mod commands;


type DbPool = Pool<ConnectionManager<SqliteConnection>>;

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!();
fn init(app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = app.path().app_local_data_dir()
        .context("failed to get app data directory")?;
    std::fs::create_dir_all(&data_dir)
        .context("failed to create data directory ")?;

    let db_path = data_dir.join("database.db");
    println!("db path: {}", db_path.display());

    let database_url = format!("file:{}", db_path.display());
    let manager = ConnectionManager::<SqliteConnection>::new(&database_url);
    let pool = Pool::builder()
        .build(manager)
        .context("Failed to create pool.")?;
    let db = Arc::new(pool);

    db.get()?
        .run_pending_migrations(MIGRATIONS)
        .expect("Error running migrations");

    app.manage(db);

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(init);

    let builder = add_handlers(builder);

    builder
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
