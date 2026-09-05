use crate::detection::{self, Command, CommandSender, NotificationReceiver};
use crate::install_scanner::InstallScannerItem;
use crate::models::*;
use crate::persistence::{GameRepository, ScanRootsRepository, VersionRepository};
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

pub struct TimelineCore {
    games_repo: Arc<dyn GameRepository>,
    versions_repo: Arc<dyn VersionRepository>,
    scan_roots_repo: Arc<dyn ScanRootsRepository>,
    cmd_tx: CommandSender,
    pub working_base: PathBuf,
    pub icons_dir: PathBuf,
    monitoring_settings_path: PathBuf,
}

impl TimelineCore {
    pub fn new(
        games_repo: Arc<dyn GameRepository>,
        versions_repo: Arc<dyn VersionRepository>,
        scan_roots_repo: Arc<dyn ScanRootsRepository>,
        working_base: PathBuf,
        icons_dir: PathBuf,
        new_version_seconds: Option<f32>,
    ) -> Result<(Self, NotificationReceiver)> {
        crate::repository::ensure_repository_root(&working_base)
            .context("Failed to initialize repository root")?;

        let monitoring_settings_path = working_base
            .parent()
            .unwrap_or(&working_base)
            .join("monitoring_settings.json");
        let mut monitoring_settings = load_monitoring_settings(&monitoring_settings_path)?;
        if let Some(seconds) = new_version_seconds {
            monitoring_settings.snapshot_debounce_seconds = seconds;
        }

        let (cmd_tx, notif_rx) = detection::init(
            games_repo.clone(),
            versions_repo.clone(),
            working_base.clone(),
            monitoring_settings,
        )?;

        Ok((
            Self {
                games_repo,
                versions_repo,
                scan_roots_repo,
                cmd_tx,
                working_base,
                icons_dir,
                monitoring_settings_path,
            },
            notif_rx,
        ))
    }

    pub fn get_games(&self) -> Result<Vec<FullGame>> {
        self.games_repo.get_games()
    }

    pub fn get_game(&self, id: i32) -> Result<FullGame> {
        self.games_repo.get_game(id)
    }

    pub fn get_slots(&self, game_id: i32) -> Result<Vec<FullSlot>> {
        self.games_repo.list_slots(game_id)
    }

    pub fn save_game(&self, form: GameForm, dirs: Option<Vec<&str>>) -> Result<Game> {
        let game = self.games_repo.save_game(form, dirs)?;
        let all_games = self.games_repo.get_games()?;
        self.cmd_tx.send(Command::ReregisterGames(all_games))?;
        Ok(game)
    }

    pub fn delete_game(&self, id: i32) -> Result<()> {
        self.games_repo.delete_game(id)?;
        let all_games = self.games_repo.get_games()?;
        self.cmd_tx.send(Command::ReregisterGames(all_games))?;
        Ok(())
    }

    pub fn tag_version(&self, id: i32, tag: String) -> Result<Version> {
        self.versions_repo.tag_version(id, tag)
    }

    pub fn create_slot(&self, game_id: i32, name: &str) -> Result<FullGame> {
        self.run_with_paused_watcher(|| {
            let current_game = self
                .games_repo
                .get_game(game_id)
                .context("Failed to load game before slot creation")?;

            self.checkpoint_current_state(&current_game)
                .context("Failed to checkpoint current slot before creating a new slot")?;

            let slot = self
                .games_repo
                .create_slot(game_id, name)
                .context("Failed to create slot")?;

            self.games_repo
                .set_active_slot(game_id, slot.slot.id)
                .context("Failed to activate new slot")?;

            let active_game = self
                .games_repo
                .get_game(game_id)
                .context("Failed to reload game after activating new slot")?;

            self.clear_tracked_dirs(&active_game)
                .context("Failed to clear tracked directories for new slot")?;

            let baseline = self
                .versions_repo
                .create_version(game_id)
                .context("Failed to create empty baseline for new slot")?;

            self.ensure_empty_snapshot_dirs(&active_game, &baseline)
                .context("Failed to initialize empty snapshot for new slot")?;

            self.resync_game_metadata(game_id)
                .context("Failed to sync metadata after creating slot")?;

            self.games_repo
                .get_game(game_id)
                .context("Failed to reload game after creating slot")
        })
    }

    pub fn switch_slot(&self, game_id: i32, slot_id: i32) -> Result<FullGame> {
        self.run_with_paused_watcher(|| {
            let current_game = self
                .games_repo
                .get_game(game_id)
                .context("Failed to load game before slot switch")?;

            if current_game
                .active_slot
                .as_ref()
                .is_some_and(|slot| slot.id == slot_id)
            {
                return Ok(current_game);
            }

            self.checkpoint_current_state(&current_game)
                .context("Failed to checkpoint current slot before switching")?;

            self.games_repo
                .set_active_slot(game_id, slot_id)
                .context("Failed to activate target slot")?;

            let target_game = self
                .games_repo
                .get_game(game_id)
                .context("Failed to reload game after switching slots")?;

            let target_slot = target_game
                .slots
                .iter()
                .find(|slot| slot.id == slot_id)
                .context("Target slot not found after switching")?;

            if let Some(active_version_id) = target_slot.active_version_id {
                let full_version = self
                    .versions_repo
                    .get_version(active_version_id)
                    .context("Failed to load active version for target slot")?;
                self.restore_version_files(&target_game, &full_version)
                    .context("Failed to restore target slot snapshot")?;
            } else {
                self.clear_tracked_dirs(&target_game)
                    .context("Failed to clear tracked directories for empty slot")?;
            }

            self.resync_game_metadata(game_id)
                .context("Failed to sync metadata after switching slots")?;

            self.games_repo
                .get_game(game_id)
                .context("Failed to reload game after switching slots")
        })
    }

    pub fn restore_version(&self, version_id: i32) -> Result<()> {
        self.run_with_paused_watcher(|| {
            let version = self
                .versions_repo
                .set_active_version(version_id)
                .context("Failed to activate version for restoration")?;
            let game = self
                .games_repo
                .get_game(version.game.id)
                .context("Failed to reload game for restoration")?;

            self.restore_version_files(&game, &version)
                .context("Failed to restore version files")?;
            self.resync_game_metadata(game.game.id)
                .context("Failed to sync metadata after restoration")?;

            println!("Restored version {} for game {}", version_id, game.game.id);

            Ok(())
        })
    }

    pub fn set_game_icon(&self, game_id: i32, source_path: &str) -> Result<String> {
        std::fs::create_dir_all(&self.icons_dir).context("Failed to create icons directory")?;

        let source = std::path::Path::new(source_path);
        let ext = source
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        let dest_path = self.icons_dir.join(format!("{}.png", game_id));

        if ext == "exe" {
            #[cfg(target_os = "windows")]
            extract_exe_icon(source, &dest_path)
                .context("Failed to extract icon from executable")?;

            #[cfg(not(target_os = "windows"))]
            anyhow::bail!("Executable icon extraction is only supported on Windows");
        } else {
            let img = image::open(source).context("Failed to open image file")?;
            img.save(&dest_path).context("Failed to save icon as PNG")?;
        }

        let icon_path = dest_path.to_string_lossy().into_owned();
        let game = self.games_repo.get_game(game_id)?;
        self.games_repo.save_game(
            GameForm {
                id: Some(game_id),
                uuid: game.game.uuid.clone(),
                title: &game.game.title,
                title_normalized: game.game.title_normalized.clone(),
                folder_slug: game.game.folder_slug.clone(),
                active_slot_id: game.game.active_slot_id,
                icon_path: Some(icon_path.clone()),
            },
            None,
        )?;

        Ok(icon_path)
    }

    pub fn get_scan_roots(&self) -> Result<Vec<String>> {
        self.scan_roots_repo.get_scan_roots()
    }

    pub fn add_scan_root(&self, path: &str) -> Result<()> {
        self.scan_roots_repo.add_scan_root(path)
    }

    pub fn remove_scan_root(&self, path: &str) -> Result<()> {
        self.scan_roots_repo.remove_scan_root(path)
    }

    pub fn get_monitoring_settings(&self) -> Result<MonitoringSettings> {
        load_monitoring_settings(&self.monitoring_settings_path)
    }

    pub fn update_monitoring_settings(
        &self,
        monitoring_settings: MonitoringSettings,
    ) -> Result<MonitoringSettings> {
        save_monitoring_settings(&self.monitoring_settings_path, monitoring_settings)?;
        self.cmd_tx
            .send(Command::UpdateMonitoringSettings(monitoring_settings))
            .context("Failed to update monitoring settings in watcher")?;
        Ok(monitoring_settings)
    }

    pub fn scan_installed(&self) -> Result<Vec<InstallScannerItem>> {
        let cache_dir = self
            .working_base
            .parent()
            .unwrap_or(&self.working_base)
            .join("cache");
        let custom_roots = self.scan_roots_repo.get_scan_roots()?;
        let mut items = crate::install_scanner::scan_installed(&cache_dir, &custom_roots)?;

        let icon_cache_dir = cache_dir.join("icon_cache");
        let _ = std::fs::create_dir_all(&icon_cache_dir);

        for item in &mut items {
            let Some(source_path) = &item.icon_path else {
                continue;
            };

            let safe_title: String = item
                .title
                .chars()
                .map(|c| {
                    if c.is_alphanumeric() || c == '-' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect();
            let cache_png = icon_cache_dir.join(format!("{safe_title}.png"));

            if cache_png.exists() {
                item.icon_path = Some(cache_png.to_string_lossy().into_owned());
                continue;
            }

            let source = std::path::Path::new(source_path);
            let ext = source
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();

            let extracted = if ext == "exe" {
                #[cfg(target_os = "windows")]
                {
                    extract_exe_icon(source, &cache_png).is_ok()
                }
                #[cfg(not(target_os = "windows"))]
                {
                    false
                }
            } else {
                image::open(source)
                    .and_then(|img| img.save(&cache_png))
                    .is_ok()
            };

            item.icon_path = if extracted {
                Some(cache_png.to_string_lossy().into_owned())
            } else {
                None
            };
        }

        Ok(items)
    }

    fn run_with_paused_watcher<T>(&self, operation: impl FnOnce() -> Result<T>) -> Result<T> {
        self.cmd_tx
            .send(Command::Pause)
            .context("Failed to send pause command")?;

        let result = operation();

        let reregister_result = self
            .games_repo
            .get_games()
            .context("Failed to reload games after mutation")
            .and_then(|all_games| {
                self.cmd_tx
                    .send(Command::ReregisterGames(all_games))
                    .context("Failed to reregister games")
            });
        let resume_result = self
            .cmd_tx
            .send(Command::Resume)
            .context("Failed to send resume command");

        match result {
            Ok(value) => {
                reregister_result?;
                resume_result?;
                Ok(value)
            }
            Err(err) => {
                let _ = reregister_result;
                let _ = resume_result;
                Err(err)
            }
        }
    }

    fn checkpoint_current_state(&self, game: &FullGame) -> Result<Option<FullVersion>> {
        if game.tracked_dirs.is_empty() {
            return Ok(None);
        }

        let version = self
            .versions_repo
            .create_version(game.game.id)
            .context("Failed to create checkpoint version")?;

        for dir in &game.tracked_dirs {
            let source_path = Path::new(&dir.path);
            let snapshot_dir = crate::repository::game_snapshot_dir(
                &self.working_base,
                &version.game,
                &version.slot,
                &version.version,
                dir,
            )
            .context("Failed to resolve checkpoint snapshot path")?;

            if snapshot_dir.exists() {
                std::fs::remove_dir_all(&snapshot_dir).with_context(|| {
                    format!("Failed to clear snapshot dir {}", snapshot_dir.display())
                })?;
            }

            if let Some(parent) = snapshot_dir.parent() {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("Failed to create snapshot parent dir {}", parent.display())
                })?;
            }

            if source_path.exists() {
                crate::fs_utils::copy_dir_filtered(source_path, &snapshot_dir)
                    .context("Failed to copy tracked dir into checkpoint snapshot")?;
            } else {
                std::fs::create_dir_all(&snapshot_dir).with_context(|| {
                    format!(
                        "Failed to create empty snapshot dir {}",
                        snapshot_dir.display()
                    )
                })?;
            }
        }

        self.resync_game_metadata(game.game.id)
            .context("Failed to sync metadata after checkpoint")?;

        Ok(Some(version))
    }

    fn clear_tracked_dirs(&self, game: &FullGame) -> Result<()> {
        for dir in &game.tracked_dirs {
            let live_dir = Path::new(&dir.path);
            if live_dir.exists() {
                std::fs::remove_dir_all(live_dir).with_context(|| {
                    format!("Failed to clear tracked dir {}", live_dir.display())
                })?;
            }

            std::fs::create_dir_all(live_dir).with_context(|| {
                format!("Failed to recreate tracked dir {}", live_dir.display())
            })?;
        }

        Ok(())
    }

    fn restore_version_files(&self, game: &FullGame, version: &FullVersion) -> Result<()> {
        for dir in &game.tracked_dirs {
            let live_dir = PathBuf::from(&dir.path);
            if live_dir.exists() {
                std::fs::remove_dir_all(&live_dir).with_context(|| {
                    format!("Failed to clear live tracked dir {}", live_dir.display())
                })?;
            }

            let snapshot_dir = crate::repository::game_snapshot_dir(
                &self.working_base,
                &game.game,
                &version.slot,
                &version.version,
                dir,
            )
            .context("Failed to resolve version snapshot path")?;

            if snapshot_dir.exists() {
                crate::fs_utils::copy_dir_filtered(&snapshot_dir, &live_dir)
                    .context("Failed to copy directory")?;
            } else {
                std::fs::create_dir_all(&live_dir).with_context(|| {
                    format!(
                        "Failed to recreate empty tracked dir {}",
                        live_dir.display()
                    )
                })?;
            }
        }

        Ok(())
    }

    fn ensure_empty_snapshot_dirs(&self, game: &FullGame, version: &FullVersion) -> Result<()> {
        for dir in &game.tracked_dirs {
            let snapshot_dir = crate::repository::game_snapshot_dir(
                &self.working_base,
                &version.game,
                &version.slot,
                &version.version,
                dir,
            )
            .context("Failed to resolve empty slot snapshot path")?;

            if snapshot_dir.exists() {
                std::fs::remove_dir_all(&snapshot_dir).with_context(|| {
                    format!("Failed to clear snapshot dir {}", snapshot_dir.display())
                })?;
            }

            std::fs::create_dir_all(&snapshot_dir).with_context(|| {
                format!(
                    "Failed to create empty snapshot dir {}",
                    snapshot_dir.display()
                )
            })?;
        }

        Ok(())
    }

    fn resync_game_metadata(&self, game_id: i32) -> Result<()> {
        let game = self
            .games_repo
            .get_game(game_id)
            .context("Failed to load game for metadata sync")?;
        self.games_repo
            .save_game(
                GameForm {
                    id: Some(game.game.id),
                    uuid: game.game.uuid.clone(),
                    title: &game.game.title,
                    title_normalized: game.game.title_normalized.clone(),
                    folder_slug: game.game.folder_slug.clone(),
                    active_slot_id: game.game.active_slot_id,
                    icon_path: game.game.icon_path.clone(),
                },
                None,
            )
            .context("Failed to persist metadata sync")?;

        Ok(())
    }
}

fn load_monitoring_settings(path: &std::path::Path) -> Result<MonitoringSettings> {
    if !path.exists() {
        let settings = MonitoringSettings::default();
        save_monitoring_settings(path, settings)?;
        return Ok(settings);
    }

    let contents = fs::read_to_string(path).context("Failed to read monitoring settings")?;
    let settings =
        serde_json::from_str(&contents).context("Failed to parse monitoring settings")?;
    Ok(settings)
}

fn save_monitoring_settings(
    path: &std::path::Path,
    monitoring_settings: MonitoringSettings,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("Failed to create monitoring settings directory")?;
    }

    let contents = serde_json::to_string_pretty(&monitoring_settings)
        .context("Failed to serialize monitoring settings")?;
    fs::write(path, contents).context("Failed to write monitoring settings")?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn extract_exe_icon(
    exe_path: &std::path::Path,
    output_path: &std::path::Path,
) -> anyhow::Result<()> {
    let script = "Add-Type -AssemblyName System.Drawing; \
                  $icon = [System.Drawing.Icon]::ExtractAssociatedIcon($env:EXE_PATH); \
                  $icon.ToBitmap().Save($env:OUT_PATH)";
    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("EXE_PATH", exe_path)
        .env("OUT_PATH", output_path)
        .status()
        .context("Failed to run PowerShell")?;
    anyhow::ensure!(status.success(), "PowerShell icon extraction failed");
    Ok(())
}
