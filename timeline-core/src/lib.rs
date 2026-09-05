#[cfg(not(any(target_os = "linux", target_os = "windows")))]
compile_error!("savegame-timeline-core supports Windows and Linux only");

pub mod content_store;
pub mod core_facade;
pub mod detection;
pub mod exclusions;
pub mod fs_utils;
pub mod install_scanner;
pub mod models;
pub mod path_placeholders;
pub mod persistence;
pub mod repository;
pub mod schema;

pub use core_facade::TimelineCore;

use diesel_migrations::{EmbeddedMigrations, embed_migrations};
pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");
