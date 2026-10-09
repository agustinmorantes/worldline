#![allow(dead_code)]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use worldline_core::models::*;
use worldline_core::sync::SyncSettings;
use worldline_core::{OpenOptions, WorldlineCore};

/// One installation: its own data directory and save folder.
pub struct Device {
    pub core: WorldlineCore,
    pub data: TempDir,
    pub saves: TempDir,
}

impl Device {
    pub fn new() -> Self {
        let data = tempfile::tempdir().unwrap();
        let saves = tempfile::tempdir().unwrap();
        let (core, _) = WorldlineCore::open(data.path(), OpenOptions::default()).unwrap();
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

/// Unzips the bundle, lets `change` edit its files, and zips it back.
pub fn edit_bundle(bundle: &Path, change: impl FnOnce(&Path)) {
    let dir = tempfile::tempdir().unwrap();
    zip::ZipArchive::new(fs::File::open(bundle).unwrap())
        .unwrap()
        .extract(dir.path())
        .unwrap();
    change(dir.path());
    let mut zip = zip::ZipWriter::new(fs::File::create(bundle).unwrap());
    let mut pending = vec![dir.path().to_path_buf()];
    while let Some(current) = pending.pop() {
        for entry in fs::read_dir(&current).unwrap() {
            let path = entry.unwrap().path();
            let name = path
                .strip_prefix(dir.path())
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let options = zip::write::SimpleFileOptions::default();
            if path.is_dir() {
                zip.add_directory(name, options).unwrap();
                pending.push(path);
            } else {
                zip.start_file(name, options).unwrap();
                zip.write_all(&fs::read(&path).unwrap()).unwrap();
            }
        }
    }
    zip.finish().unwrap();
}
