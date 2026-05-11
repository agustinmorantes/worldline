pub mod core_facade;
pub mod detection;
pub mod fs_utils;
pub mod install_scanner;
pub mod models;
pub mod persistence;
pub mod schema;

pub use core_facade::TimelineCore;

use diesel_migrations::{EmbeddedMigrations, embed_migrations};
pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");
