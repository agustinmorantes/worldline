use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use crate::detection::{CommandSender, Notification, NotificationSender};
use anyhow::{Context, Result};
use notify_debouncer_full::DebouncedEvent;
use crate::DbPool;
use crate::models::{FullGame, FullVersion, TrackedDir};
use crate::persistence::versions::create_version;
use crate::persistence::games::get_games;

const NEW_VERSION_SECONDS: f32 = 60f32 * 5f32; // 5 minutes

pub struct FsEventHandler {
    working_base: PathBuf,
    db: DbPool,
    cmd_tx: CommandSender,
}

impl FsEventHandler {
    pub fn new(working_base: PathBuf, db: DbPool, cmd_tx: CommandSender) -> Self {
        Self { working_base, db, cmd_tx }
    }

    pub fn run(
        &self,
        events: Vec<DebouncedEvent>,
        notif_tx: NotificationSender,
        games: &[FullGame],
        working_base: &Path,
    ) -> Result<()> {
        let dirs = games.iter()
            .flat_map(|game| game.tracked_dirs.iter())
            .cloned()
            .collect::<Vec<TrackedDir>>();

        let first_scan_dirs = games.iter()
            .flat_map(|game| game.tracked_dirs.iter().map(move |dir| (game, dir)))
            .filter(|(game, dir)| {
                let working_dir_path = &working_base
                    .join(game.game.id.to_string())
                    .join(game.game.active_version_id.unwrap_or(1).to_string())
                    .join(dir.id.to_string());
                !working_dir_path.exists()
            })
            .map(|(_, dir)| dir)
            .cloned()
            .collect::<Vec<TrackedDir>>();

        self.handle_rescan(&first_scan_dirs, games).ok();

        // Collect all modified tracked directories
        let mut modified_dirs: HashSet<i32> = HashSet::new();
        let mut modified_games: HashSet<i32> = HashSet::new();

        for ev in events {
            println!("event detected: {:?}", ev);

            if ev.need_rescan() {
                return self.handle_rescan(&dirs, games);
            }

            // Find which tracked directories were affected by this event
            for path in &ev.paths {
                for dir in &dirs {
                    if path.starts_with(&dir.path) {
                        modified_dirs.insert(dir.id);
                        modified_games.insert(dir.game_id);
                    }
                }
            }
        }

        // Create new versions for modified games BEFORE syncing
        // This ensures that new changes go into the new version directory
        let mut new_versions: HashSet<i32> = HashSet::new();
        for game_id in &modified_games {
            let game = games.iter()
                .find(|g| g.game.id == *game_id)
                .context("failed to find modified game")?;

            if let Some(version) = self.handle_new_version(game)
                .context("failed to handle new version")?
            {
                new_versions.insert(*game_id);

                // Send notification about new version
                notif_tx.send(Notification::VersionCreated {
                    game_id: *game_id,
                    version_number: version.version.version_number
                })?;
            }
        }

        // Reload games if new versions were created to get updated active_version_id
        let games = if !new_versions.is_empty() {
            self.reload_games(games)?
        } else {
            games.to_vec()
        };

        // Now copy all modified tracked directories into the correct version
        let dirs_to_sync: Vec<&TrackedDir> = dirs.iter()
            .filter(|dir| modified_dirs.contains(&dir.id))
            .collect();

        for dir in &dirs_to_sync {
            self.sync_tracked_dir(dir, &games)
                .unwrap_or_else(|e| eprintln!("Error syncing tracked dir {}: {}", dir.id, e));
        }

        // Send change detected notifications
        for game_id in &modified_games {
            notif_tx.send(Notification::ChangeDetected {
                game_id: *game_id,
            })?;
        }

        Ok(())
    }

    fn sync_tracked_dir(&self, dir: &TrackedDir, games: &[FullGame]) -> Result<()> {
        let Some(game) = games.iter()
            .find(|g| g.game.id == dir.game_id)
        else {
            return Ok(());
        };

        let source_path = Path::new(&dir.path);
        let working_dir_path = &self.working_base
            .join(game.game.id.to_string())
            .join(game.game.active_version_id.unwrap_or(1).to_string())
            .join(dir.id.to_string());

        // Remove the existing directory if it exists
        if working_dir_path.exists() {
            fs::remove_dir_all(working_dir_path)?;
        }

        // Ensure parent directory exists
        if let Some(parent) = working_dir_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        // Copy the entire directory
        let mut options = fs_extra::dir::CopyOptions::new();
        options.copy_inside = true;
        options.overwrite = true;
        fs_extra::dir::copy(source_path, working_dir_path, &options)?;

        Ok(())
    }

    fn handle_rescan(&self, dirs: &[TrackedDir], games: &[FullGame]) -> Result<()> {
        for dir in dirs {
            let Some(game) = games.iter()
                .find(|g| g.game.id == dir.game_id)
            else {
                continue;
            };

            let path = Path::new(&dir.path);
            let working_dir_path = &self.working_base
                .join(game.game.id.to_string())
                .join(game.game.active_version_id.unwrap_or(1).to_string())
                .join(dir.id.to_string());

            if working_dir_path.exists() {
                fs::remove_dir_all(working_dir_path)?;
            }
            else if !working_dir_path.parent().unwrap().exists() {
                fs::create_dir_all(working_dir_path.parent().unwrap())?;
            }

            let mut options = fs_extra::dir::CopyOptions::new();
            options.copy_inside = true;
            options.overwrite = true;
            fs_extra::dir::copy(path, working_dir_path, &options)?;
        }

        Ok(())
    }

    fn reload_games(&self, _old_games: &[FullGame]) -> Result<Vec<FullGame>> {
        // Reload all games from database to get updated active_version_id
        get_games(self.db.clone())
            .context("failed to reload games from database")
    }

    fn handle_new_version(&self, game: &FullGame) -> Result<Option<FullVersion>> {
        let last_version = &game.last_version;

        let mut create_new_version = false;
        if let Some(last_version) = last_version {
            // Check if enough time has passed since last version
            if chrono::Utc::now().signed_duration_since(last_version.created_at.and_utc()).as_seconds_f32() > NEW_VERSION_SECONDS {
                create_new_version = true
            }
        } else {
            create_new_version = true
        }

        if !create_new_version {
            return Ok(None);
        }

        println!("Creating new game version");

        let version = create_version(game.game.id, self.db.clone(), self.cmd_tx.clone())
            .context("failed to create new version")?;

        println!("Created new version with id: {}", version.version.id);

        Ok(Some(version))
    }
}
