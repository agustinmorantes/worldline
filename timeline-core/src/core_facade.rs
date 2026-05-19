use crate::detection::{self, Command, CommandSender, NotificationReceiver};
use crate::install_scanner::InstallScannerItem;
use crate::models::*;
use crate::persistence::{GameRepository, ScanRootsRepository, VersionRepository};
use anyhow::{Context, Result};
use std::fs;
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

    pub fn restore_version(&self, version_id: i32) -> Result<()> {
        self.cmd_tx
            .send(Command::Pause)
            .context("Failed to send pause command")?;

        let version = self
            .versions_repo
            .get_version(version_id)
            .context("Failed to get version")?;
        let game = self
            .games_repo
            .get_game(version.game.id)
            .context("Failed to get game")?;
        let dirs = &game.tracked_dirs;

        // Update active_version_id to the restored version ID
        // This effectively sets the "parent" for the next auto-save
        self.games_repo
            .save_game(
                GameForm {
                    id: Some(game.game.id),
                    title: &game.game.title,
                    active_version_id: Some(version_id),
                    icon_path: None,
                },
                None,
            )
            .context("Failed to update active_version_id for restoration")?;

        let paths: Vec<_> = dirs.iter().map(|dir| dir.path.as_str()).collect();

        for path in &paths {
            let _ = fs_extra::remove_items(&[*path]);
        }

        for dir in dirs {
            let from = self
                .working_base
                .join(game.game.id.to_string())
                .join(version.version.id.to_string())
                .join(dir.id.to_string());

            let to = std::path::PathBuf::from(&dir.path);

            crate::fs_utils::copy_dir_filtered(&from, &to).context("Failed to copy directory")?;
        }

        let all_games = self.games_repo.get_games()?;
        self.cmd_tx
            .send(Command::ReregisterGames(all_games))
            .context("Failed to reregister games")?;

        self.cmd_tx
            .send(Command::Resume)
            .context("Failed to send resume command")?;

        println!("Restored version {} for game {}", version_id, game.game.id);

        Ok(())
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
                title: &game.game.title,
                active_version_id: game.game.active_version_id,
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
                .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' })
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
}

fn load_monitoring_settings(path: &std::path::Path) -> Result<MonitoringSettings> {
    if !path.exists() {
        let settings = MonitoringSettings::default();
        save_monitoring_settings(path, settings)?;
        return Ok(settings);
    }

    let contents = fs::read_to_string(path).context("Failed to read monitoring settings")?;
    let settings = serde_json::from_str(&contents).context("Failed to parse monitoring settings")?;
    Ok(settings)
}

fn save_monitoring_settings(path: &std::path::Path, monitoring_settings: MonitoringSettings) -> Result<()> {
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
