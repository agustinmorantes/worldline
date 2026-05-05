pub mod sqlite;

use crate::models::{FullGame, Game, GameForm, Version};
use anyhow::Result;

#[cfg_attr(test, mockall::automock)]
pub trait GameRepository: Send + Sync {
    fn get_games(&self) -> Result<Vec<FullGame>>;
    fn get_game(&self, id: i32) -> Result<FullGame>;
    fn save_game<'a>(&self, game: GameForm<'a>, dirs: Option<Vec<&'a str>>) -> Result<Game>;
    fn delete_game(&self, id: i32) -> Result<()>;
}

#[cfg_attr(test, mockall::automock)]
pub trait VersionRepository: Send + Sync {
    fn create_version(&self, game_id: i32) -> Result<crate::models::FullVersion>;
    fn tag_version(&self, id: i32, tag: String) -> Result<Version>;
    fn get_version(&self, id: i32) -> Result<crate::models::FullVersion>;
}
