use crate::detection::{self, Command, CommandSender, NotificationReceiver};
use crate::install_scanner::InstallScannerItem;
use crate::models::*;
use crate::persistence::{GameRepository, ScanRootsRepository, VersionRepository};
use crate::repository::VersionReason;
use anyhow::{Context, Result, anyhow};
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

/// How long a caller waits for the watcher to acknowledge a pause before giving
/// up and proceeding unsuppressed. Generous, because it should only ever be hit
/// by a wedged watcher thread: a dead one drops the acknowledgement channel and
/// fails the wait immediately. A spurious version is a far better outcome than a
/// restore that hangs the UI.
const WATCHER_PAUSE_ACK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// How long to stay paused after mutating tracked directories, so the OS has time
/// to deliver events for our own writes while they are still being discarded.
/// Chosen to comfortably exceed filesystem notification latency without being
/// noticeable in a restore; the debouncer is rebuilt on `Resume` as well, which
/// covers anything still buffered when this elapses.
const WATCHER_SETTLE_DELAY: std::time::Duration = std::time::Duration::from_millis(250);

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

            self.checkpoint_current_state(&current_game, VersionReason::PreSlotCreate)
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

            let empty_snapshot = crate::content_store::capture_snapshot(&self.working_base, &[])
                .context("Failed to capture empty baseline")?;
            self.versions_repo
                .create_version_from_snapshot(game_id, VersionReason::Initial, empty_snapshot)
                .context("Failed to create empty baseline for new slot")?;

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

            self.checkpoint_current_state(&current_game, VersionReason::PreSwitch)
                .context("Failed to checkpoint current slot before switching")?;

            self.activate_slot(game_id, slot_id)
        })
    }

    /// Makes `slot_id` the game's active slot and materialises its files into
    /// the live tracked directories. The caller owns the decision of whether
    /// the outgoing slot's live state is worth checkpointing first, and must
    /// have the watcher paused: this writes to tracked directories.
    fn activate_slot(&self, game_id: i32, slot_id: i32) -> Result<FullGame> {
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
    }

    pub fn rename_slot(&self, slot_id: i32, name: &str) -> Result<FullGame> {
        let slot = self
            .games_repo
            .rename_slot(slot_id, name)
            .context("Failed to rename slot")?;

        self.games_repo
            .get_game(slot.slot.game_id)
            .context("Failed to reload game after renaming slot")
    }

    /// Deletes a slot and the whole version history it owns.
    ///
    /// A game with no active slot has no coherent state, so two rules are
    /// enforced here rather than in the UI: the last remaining slot can never
    /// be deleted, and deleting the *active* slot requires naming the slot to
    /// switch to first. The replacement is switched into (restoring its files)
    /// before the target is dropped, so the live save directory always ends up
    /// holding some slot's real state.
    pub fn delete_slot(&self, slot_id: i32, replacement_slot_id: Option<i32>) -> Result<FullGame> {
        let slot = self
            .games_repo
            .get_slot(slot_id)
            .context("Failed to load slot for deletion")?;
        let game_id = slot.slot.game_id;
        let game = self
            .games_repo
            .get_game(game_id)
            .context("Failed to load game before slot deletion")?;

        if game.slots.len() <= 1 {
            return Err(anyhow!(
                "Cannot delete '{}': a game must keep at least one slot",
                slot.slot.name
            ));
        }

        let is_active = game
            .active_slot
            .as_ref()
            .is_some_and(|active| active.id == slot_id);

        if is_active {
            let replacement_slot_id = replacement_slot_id.context(
                "Deleting the active slot requires a replacement slot to switch to first",
            )?;
            if replacement_slot_id == slot_id {
                return Err(anyhow!(
                    "The replacement slot must not be the slot being deleted"
                ));
            }
            if !game
                .slots
                .iter()
                .any(|candidate| candidate.id == replacement_slot_id)
            {
                return Err(anyhow!(
                    "Replacement slot {replacement_slot_id} does not belong to this game"
                ));
            }

            // Deliberately no checkpoint of the outgoing slot: it is about to be
            // discarded, so capturing its live state would only mint a version
            // that the delete below immediately removes.
            self.run_with_paused_watcher(|| self.activate_slot(game_id, replacement_slot_id))
                .context("Failed to switch to the replacement slot before deleting")?;
        }

        // The slot's directory under the repository is deliberately left on
        // disk, matching `delete_game`. Nothing in the app can bring a deleted
        // timeline back, so keeping the bytes is the reversible failure mode;
        // orphaned slot directories accumulate until a repository GC pass
        // reclaims them.
        self.games_repo
            .delete_slot(slot_id)
            .context("Failed to delete slot")?;

        self.games_repo
            .get_game(game_id)
            .context("Failed to reload game after deleting slot")
    }

    pub fn restore_version(&self, version_id: i32) -> Result<()> {
        self.run_with_paused_watcher(|| {
            // Read and validate BEFORE any filesystem mutation.
            let version = self
                .versions_repo
                .get_version(version_id)
                .context("Failed to load version for restoration")?;
            let game = self
                .games_repo
                .get_game(version.game.id)
                .context("Failed to reload game for restoration")?;

            if !game
                .slots
                .iter()
                .any(|slot| slot.id == version.version.slot_id)
            {
                return Err(anyhow!(
                    "Version {} does not belong to game {}",
                    version_id,
                    game.game.id
                ));
            }

            self.versions_repo
                .set_active_version(version_id)
                .context("Failed to activate version for restoration")?;
            self.restore_version_files(&game, &version)
                .context("Failed to restore version files")?;
            self.resync_game_metadata(game.game.id)
                .context("Failed to sync metadata after restoration")?;

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

    /// Stops the watcher thread and everything it owns (the debouncer and its
    /// notification threads). Nothing else ever ends a watcher's life, so a
    /// process that builds several cores accumulates live debouncers, each one
    /// ticking and delivering events for directories nobody is looking at.
    pub fn shutdown(&self) {
        let _ = self.cmd_tx.send(Command::Shutdown);
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

    /// Runs `operation` with the watcher suppressed, so the writes it makes to
    /// tracked directories are not mistaken for the user playing the game.
    ///
    /// The contract is: pause and wait for the watcher to confirm it, mutate, let
    /// trailing OS events arrive while still paused, resume.
    /// Every wait is bounded and the watcher is always resumed — including when
    /// `operation` returns `Err` or panics, since leaving suppression latched
    /// would silently stop versioning for the rest of the session. A watcher that
    /// is gone or wedged costs suppression, never the operation itself.
    fn run_with_paused_watcher<T>(&self, operation: impl FnOnce() -> Result<T>) -> Result<T> {
        let (ack_tx, ack_rx) = std::sync::mpsc::channel();
        // A stopped watcher must not make restoring impossible. Propagating here
        // would fail every restore and slot switch for the rest of the session;
        // a spurious version is the better failure by a wide margin, so this logs
        // and falls through. The wait below then fails immediately (the ack sender
        // went with the dropped command) and the operation runs unsuppressed.
        if let Err(err) = self.cmd_tx.send(Command::Pause(ack_tx)) {
            eprintln!("Watcher is not accepting commands ({err}); continuing unsuppressed");
        }

        // Block until the watcher confirms it has seen the pause. Without this the
        // operation races an event the debouncer has already promoted to the queue
        // but the watcher has not dequeued yet: that event is still judged unpaused
        // and becomes a version. Rebuilding the debouncer on `Resume` cannot help
        // there — it only discards what is still buffered inside the debouncer.
        // Bounded: a watcher thread that died drops the ack sender and this returns
        // at once, and one that is wedged costs a bounded wait instead of hanging
        // the UI forever. Either way suppression degrades rather than deadlocks.
        if let Err(err) = ack_rx.recv_timeout(WATCHER_PAUSE_ACK_TIMEOUT) {
            // Distinguishes a wedged watcher (timed out) from a stopped one
            // (sending half closed), which are very different things to debug.
            eprintln!("Watcher did not acknowledge pause ({err}); continuing unsuppressed");
        }

        // `catch_unwind` so a panicking operation still reaches the resume below.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation));

        // Let trailing OS events for our own writes arrive while still paused, so
        // they are discarded instead of being delivered once watching resumes.
        std::thread::sleep(WATCHER_SETTLE_DELAY);

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
            Ok(Ok(value)) => {
                reregister_result?;
                resume_result?;
                Ok(value)
            }
            Ok(Err(err)) => {
                let _ = reregister_result;
                let _ = resume_result;
                Err(err)
            }
            // The watcher has been resumed; let the panic carry on unchanged.
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }

    fn checkpoint_current_state(
        &self,
        game: &FullGame,
        reason: VersionReason,
    ) -> Result<Option<FullVersion>> {
        if game.tracked_dirs.is_empty() {
            return Ok(None);
        }

        let outcome = crate::checkpoint::checkpoint_current_state(
            &self.working_base,
            self.versions_repo.as_ref(),
            game,
            reason,
        )?;
        let version = match outcome {
            crate::checkpoint::CheckpointOutcome::Unchanged(version)
            | crate::checkpoint::CheckpointOutcome::Created(version) => version,
        };
        Ok(Some(version))
    }

    fn clear_tracked_dirs(&self, game: &FullGame) -> Result<()> {
        for dir in &game.tracked_dirs {
            let live_dir = Path::new(&dir.path);
            if live_dir.exists() {
                crate::fs_utils::clear_dir_except(
                    live_dir,
                    &crate::exclusions::parse(&dir.exclusions),
                )
                .with_context(|| format!("Failed to clear tracked dir {}", live_dir.display()))?;
            }

            std::fs::create_dir_all(live_dir).with_context(|| {
                format!("Failed to recreate tracked dir {}", live_dir.display())
            })?;
        }

        Ok(())
    }

    fn restore_version_files(&self, game: &FullGame, version: &FullVersion) -> Result<()> {
        let game_folder_slug = game
            .game
            .folder_slug
            .as_deref()
            .context("Game is missing a folder slug")?;
        let version_folder_slug = version
            .version
            .folder_slug
            .as_deref()
            .context("Version is missing a folder slug")?;
        let version_dir = crate::repository::version_dir(
            &crate::repository::slot_dir(
                &crate::repository::game_dir(&self.working_base, game_folder_slug),
                &version.slot.folder_slug,
            ),
            version_folder_slug,
        );
        let metadata: crate::repository::VersionMetadata =
            crate::repository::read_metadata_json(&version_dir)
                .context("Failed to read version metadata")?;
        let validated_files =
            crate::content_store::validate_snapshot_blobs(&self.working_base, &metadata)
                .context("Failed to validate snapshot manifest")?;
        let mut files = Vec::new();
        for (file, blob) in validated_files {
            let tracked_dir = game
                .tracked_dirs
                .iter()
                .find(|dir| {
                    dir.uuid
                        .as_deref()
                        .and_then(|uuid| uuid::Uuid::parse_str(uuid).ok())
                        == Some(file.tracked_dir_uuid)
                })
                .with_context(|| {
                    format!(
                        "Version references unknown tracked directory {}",
                        file.tracked_dir_uuid
                    )
                })?;
            let relative = Path::new(&file.relative_path);
            if crate::exclusions::is_excluded(
                &crate::exclusions::parse(&tracked_dir.exclusions),
                relative,
            ) {
                continue;
            }
            files.push((
                PathBuf::from(&tracked_dir.path),
                relative.to_path_buf(),
                blob,
            ));
        }

        for dir in &game.tracked_dirs {
            let live_dir = PathBuf::from(&dir.path);
            let exclusions = crate::exclusions::parse(&dir.exclusions);
            if live_dir.exists() {
                crate::fs_utils::clear_dir_except(&live_dir, &exclusions).with_context(|| {
                    format!("Failed to clear live tracked dir {}", live_dir.display())
                })?;
            }

            std::fs::create_dir_all(&live_dir).with_context(|| {
                format!(
                    "Failed to recreate empty tracked dir {}",
                    live_dir.display()
                )
            })?;
        }

        for (live_dir, relative, blob) in files {
            let destination = live_dir.join(relative);
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("Failed to create restore directory {}", parent.display())
                })?;
            }
            std::fs::copy(&blob, &destination).with_context(|| {
                format!(
                    "Failed to restore blob {} to {}",
                    blob.display(),
                    destination.display()
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
