pub mod models;
pub mod schema;
pub mod persistence;
pub mod detection;
pub mod core_facade;
pub mod install_scanner;

pub use core_facade::TimelineCore;

use diesel_migrations::{embed_migrations, EmbeddedMigrations};
pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");
