#![allow(dead_code)]

use savegame_timeline_core::models::*;
use savegame_timeline_core::sync::SyncSettings;
use savegame_timeline_core::{OpenOptions, TimelineCore};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// One installation: its own data directory and save folder.
pub struct Device {
    pub core: TimelineCore,
    pub data: TempDir,
    pub saves: TempDir,
}

impl Device {
    pub fn new() -> Self {
        let data = tempfile::tempdir().unwrap();
        let saves = tempfile::tempdir().unwrap();
        let (core, _) = TimelineCore::open(data.path(), OpenOptions::default()).unwrap();
        Self { core, data, saves }
    }

    /// A device using `remote` as its sync folder.
    pub fn synced(remote: &Path) -> Self {
        let device = Self::new();
        device
            .core
            .update_sync_settings(SyncSettings {
                remote_path: Some(remote.to_string_lossy().into_owned()),
            })
            .unwrap();
        device
    }

    pub fn save_dir(&self) -> PathBuf {
        self.saves.path().join("Game Saves")
    }

    pub fn write(&self, name: &str, contents: &str) {
        let path = self.save_dir().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    pub fn read(&self, name: &str) -> Option<String> {
        fs::read_to_string(self.save_dir().join(name)).ok()
    }

    /// Creates a game tracking `save_dir()`; its current saves become version 1.
    pub fn add_game(&self, title: &str) -> FullGame {
        fs::create_dir_all(self.save_dir()).unwrap();
        let game = self
            .core
            .save_game(
                GameForm {
                    id: None,
                    title: title.into(),
                },
                Some(vec![self.save_dir().to_string_lossy().into_owned()]),
            )
            .unwrap();
        self.core.get_game(game.id).unwrap()
    }

    pub fn game(&self, id: i32) -> FullGame {
        self.core.get_game(id).unwrap()
    }

    pub fn game_by_title(&self, title: &str) -> Option<FullGame> {
        self.core
            .get_games()
            .unwrap()
            .into_iter()
            .find(|g| g.game.title == title)
    }

    pub fn active_version(&self, id: i32) -> Version {
        self.game(id)
            .last_version
            .expect("game has an active version")
    }

    /// Checkpoints and returns the active version afterwards.
    pub fn checkpoint(&self, id: i32) -> Version {
        self.core
            .checkpoint(id)
            .unwrap()
            .version()
            .expect("a version")
            .clone()
    }
}

impl Device {
    /// Tracks another device's save folder (both "machines" share one disk in tests).
    pub fn add_game_at(&self, title: &str, dir: &Path) -> FullGame {
        let game = self
            .core
            .save_game(
                GameForm {
                    id: None,
                    title: title.into(),
                },
                Some(vec![dir.to_string_lossy().into_owned()]),
            )
            .unwrap();
        self.core.get_game(game.id).unwrap()
    }
}
