#[cfg(not(any(target_os = "linux", target_os = "windows")))]
compile_error!("worldline-core supports Windows and Linux only");

pub mod bundle;
pub mod chunks;
pub mod core_facade;
pub mod detection;
pub mod error;
pub mod exclusions;
pub mod fs_utils;
pub mod history;
pub mod icon;
pub mod index;
pub mod install_scanner;
pub mod integrity;
pub mod maintenance;
pub mod models;
pub mod path_placeholders;
pub mod restore;
pub mod schema;
#[cfg(windows)]
pub mod screenshot;
#[cfg(not(windows))]
#[path = "screenshot_unsupported.rs"]
pub mod screenshot;
pub mod store;
pub mod sync;
mod sync_cache;
mod sync_loop;
pub mod transfer;

pub use core_facade::{OpenOptions, WorldlineCore};

use diesel_migrations::{EmbeddedMigrations, embed_migrations};
pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");
