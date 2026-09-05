pub mod sqlite;

use crate::models::{FullGame, FullSlot, FullVersion, Game, GameForm, Slot, Version};
use crate::repository::VersionReason;
use anyhow::Result;

#[cfg_attr(test, mockall::automock)]
pub trait GameRepository: Send + Sync {
    fn get_games(&self) -> Result<Vec<FullGame>>;
    fn get_game(&self, id: i32) -> Result<FullGame>;
    fn get_slot(&self, id: i32) -> Result<FullSlot>;
    fn list_slots(&self, game_id: i32) -> Result<Vec<FullSlot>>;
    fn create_slot(&self, game_id: i32, name: &str) -> Result<FullSlot>;
    fn rename_slot(&self, slot_id: i32, name: &str) -> Result<FullSlot>;
    /// Removes a slot together with every version it owns. Callers are
    /// responsible for ensuring the slot is neither the game's last one nor
    /// its currently active one.
    fn delete_slot(&self, slot_id: i32) -> Result<()>;
    fn set_active_slot(&self, game_id: i32, slot_id: i32) -> Result<Slot>;
    fn save_game<'a>(&self, game: GameForm<'a>, dirs: Option<Vec<&'a str>>) -> Result<Game>;
    fn delete_game(&self, id: i32) -> Result<()>;
}

#[cfg_attr(test, mockall::automock)]
pub trait VersionRepository: Send + Sync {
    fn create_version(&self, game_id: i32, reason: VersionReason) -> Result<FullVersion>;
    fn set_active_version(&self, version_id: i32) -> Result<FullVersion>;
    fn tag_version(&self, id: i32, tag: String) -> Result<Version>;
    fn get_version(&self, id: i32) -> Result<FullVersion>;
}

#[cfg_attr(test, mockall::automock)]
pub trait ScanRootsRepository: Send + Sync {
    fn get_scan_roots(&self) -> Result<Vec<String>>;
    fn add_scan_root(&self, path: &str) -> Result<()>;
    fn remove_scan_root(&self, path: &str) -> Result<()>;
}
