use crate::detection::{self, Command, CommandSender, NotificationReceiver};
use crate::install_scanner::InstallScannerItem;
use crate::models::*;
use crate::persistence::{GameRepository, ScanRootsRepository, VersionRepository};
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::Arc;

pub struct TimelineCore {
    games_repo: Arc<dyn GameRepository>,
    versions_repo: Arc<dyn VersionRepository>,
    scan_roots_repo: Arc<dyn ScanRootsRepository>,
    cmd_tx: CommandSender,
    pub working_base: PathBuf,
    pub icons_dir: PathBuf,
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
        let (cmd_tx, notif_rx) = detection::init(
            games_repo.clone(),
            versions_repo.clone(),
            working_base.clone(),
            new_version_seconds.unwrap_or(30.0),
        )?;

        Ok((
            Self {
                games_repo,
                versions_repo,
                scan_roots_repo,
                cmd_tx,
                working_base,
                icons_dir,
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
                .join(version.version.version_number.to_string())
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

    pub fn scan_installed(&self) -> Result<Vec<InstallScannerItem>> {
        let cache_dir = self
            .working_base
            .parent()
            .unwrap_or(&self.working_base)
            .join("cache");
        let custom_roots = self.scan_roots_repo.get_scan_roots()?;
        crate::install_scanner::scan_installed(&cache_dir, &custom_roots)
    }
}

#[cfg(target_os = "windows")]
fn extract_exe_icon(
    exe_path: &std::path::Path,
    output_path: &std::path::Path,
) -> anyhow::Result<()> {
    let exe = exe_path.to_string_lossy();
    let out = output_path.to_string_lossy();
    let script = format!(
        "Add-Type -AssemblyName System.Drawing; \
         $icon = [System.Drawing.Icon]::ExtractAssociatedIcon('{exe}'); \
         $icon.ToBitmap().Save('{out}')"
    );
    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .status()
        .context("Failed to run PowerShell")?;
    anyhow::ensure!(status.success(), "PowerShell icon extraction failed");
    Ok(())
}
