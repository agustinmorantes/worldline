use crate::bundle::{HistoryBundleSummary, HistoryTransferReport};
use crate::detection::running::RunningMonitor;
use crate::detection::{
    CaptureTarget, Checkpoints, Notification, NotificationReceiver, RunningGames, Suppression,
    Watcher,
};
use crate::error::ErrorKind;
use crate::history::{CheckpointOutcome, History, game_uuid};
use crate::index::{DbPool, Index};
use crate::install_scanner::InstallScannerItem;
use crate::maintenance::{self, GcReport, RetentionSettings};
use crate::models::*;
use crate::restore;
use crate::store::{self, GameRecord, Store, VersionReason, read_json, write_json};
use crate::sync::{
    self, DeviceKind, DeviceMetadata, DeviceState, Divergence, Remote, SyncReport, SyncSettings,
};
use crate::sync_loop::{SyncLoop, Trigger};
use crate::transfer::{Completeness, Heal, Transfer};
use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Utc};
use diesel::SqliteConnection;
use diesel::r2d2::{ConnectionManager, Pool};
use diesel_migrations::MigrationHarness;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, RwLock, mpsc};
use uuid::Uuid;

/// Automatic pruning runs at most this often.
const PRUNE_INTERVAL: chrono::Duration = chrono::Duration::hours(4);

#[derive(Debug, Clone, Default)]
pub struct OpenOptions {
    /// Start the watcher (GUI and `watch`); one-shot CLI commands don't.
    pub watch: bool,
    /// Overrides the debounce from the monitoring settings (tests).
    pub debounce_seconds: Option<f32>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PruningPreview {
    pub retire_versions: Vec<Uuid>,
    pub live_bytes_before: u64,
    pub live_bytes_after: u64,
    pub budget_unreachable: bool,
    /// What a local garbage collection would reclaim right now.
    pub local_gc: GcReport,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PruningRunReport {
    pub retired_versions: Vec<Uuid>,
    pub budget_unreachable: bool,
    pub local_gc: GcReport,
    pub remote_gc: Option<GcReport>,
}

impl PruningRunReport {
    pub fn changed_history(&self) -> bool {
        !self.retired_versions.is_empty()
            || self.local_gc.chunks_deleted > 0
            || self
                .remote_gc
                .as_ref()
                .is_some_and(|gc| gc.chunks_deleted > 0)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SyncState {
    last_full_heal: Option<DateTime<Utc>>,
    /// Versions copied from the remote lately, so the next pushes heal them
    /// like new ones however old they are.
    #[serde(default)]
    recent_pulls: HashMap<Uuid, DateTime<Utc>>,
}

/// How far back a push heals versions that were created or pulled.
fn heal_window() -> chrono::Duration {
    maintenance::GRACE * 2
}

/// State shared with the watcher thread.
struct Inner {
    history: History,
    suppression: Suppression,
    /// Where writes to live saves keep their transactions (ADR 0008).
    txn_root: PathBuf,
    /// Asks the sync loop to push; ignored when no loop runs.
    sync_trigger: mpsc::Sender<Trigger>,
    /// Tells the watcher, when it runs, that a checkpoint completed.
    checkpoints: OnceLock<Checkpoints>,
}

impl Inner {
    /// The one way to capture live saves: a created version queues a push,
    /// and the watcher hears of every outcome (for screenshots).
    /// First it finishes putting back saves a failed write left half
    /// written; while that can't finish, the game isn't captured, so the
    /// half-written folder is never recorded, and the watcher retries.
    fn checkpoint(&self, game: &FullGame, reason: VersionReason) -> Result<CheckpointOutcome> {
        let recovery = restore::recover(&self.txn_root)?;
        if recovery.pending.iter().any(|root| {
            game.tracked_dirs
                .iter()
                .any(|dir| Path::new(&dir.path) == root)
        }) {
            return Err(anyhow::Error::new(ErrorKind::RestoreIncomplete)
                .context("The saves are still being put back"));
        }
        let started = chrono::Utc::now().naive_utc();
        let outcome = self.history.checkpoint_locked(game, reason)?;
        if matches!(outcome, CheckpointOutcome::Created(_)) {
            let _ = self.sync_trigger.send(Trigger::Push);
        }
        if let Some(checkpoints) = self.checkpoints.get() {
            checkpoints.completed(game.game.id, started, &outcome);
        }
        Ok(outcome)
    }
}

impl CaptureTarget for Inner {
    /// Imported history waits for the user to choose a version: capturing
    /// its folder first would start a second, unrelated root.
    fn games(&self) -> Result<Vec<FullGame>> {
        let mut games = self.history.index.get_games()?;
        games.retain(|game| game.last_version.is_some() || game.versions.is_empty());
        Ok(games)
    }

    fn capture(&self, game_id: i32) -> Result<CheckpointOutcome> {
        store::with_lock(self.history.root(), || {
            let game = self.history.index.get_game(game_id)?;
            self.checkpoint(&game, VersionReason::Watcher)
        })
    }

    fn attach_screenshot(&self, version_id: i32, jpeg: &[u8]) -> Result<()> {
        store::with_lock(self.history.root(), || {
            self.history.write_screenshot(version_id, jpeg)
        })
    }
}

pub struct WorldlineCore {
    inner: Arc<Inner>,
    watcher: Mutex<Option<Watcher>>,
    sync_loop: Mutex<Option<SyncLoop>>,
    running: Mutex<Option<RunningMonitor>>,
    /// What `running` last saw.
    running_games: RunningGames,
    pub data_dir: PathBuf,
    device: RwLock<DeviceMetadata>,
    sync_lock: Mutex<()>,
    /// Installed titles seen by the last full sync, for later reports.
    last_installs: Mutex<HashMap<String, Option<String>>>,
    /// Install dirs by normalized title, from the last install scan.
    scanned_installs: Mutex<HashMap<String, Option<String>>>,
    last_prune: Mutex<Option<DateTime<Utc>>>,
    moved_old_data: Option<PathBuf>,
    /// Writes to live saves a crash interrupted, rolled back on this start.
    recovered_restores: usize,
}

impl WorldlineCore {
    /// Opens (creating when needed) the data directory used by one-shot CLI
    /// commands. No sync loop runs (the watcher does only when `options.watch` is set).
    pub fn open(data_dir: &Path, options: OpenOptions) -> Result<(Self, NotificationReceiver)> {
        let (core, notifications, ..) = Self::open_parts(data_dir, options)?;
        Ok((core, notifications))
    }

    /// Opens for a long-running front end. With `options.watch` it also runs
    /// the watcher and the sync loop: history reaches the sync folder as it is
    /// created, other devices' progress is picked up, pruning runs when due.
    pub fn open_shared(
        data_dir: &Path,
        options: OpenOptions,
    ) -> Result<(Arc<Self>, NotificationReceiver)> {
        let watch = options.watch;
        let (core, notifications, triggers, tx) = Self::open_parts(data_dir, options)?;
        let core = Arc::new(core);
        if watch {
            *core.sync_loop.lock().unwrap_or_else(|e| e.into_inner()) = Some(SyncLoop::start(
                Arc::downgrade(&core),
                triggers,
                core.inner.sync_trigger.clone(),
                tx,
            ));
        }
        Ok((core, notifications))
    }

    /// The core, plus what `open_shared` needs to start the sync loop.
    fn open_parts(
        data_dir: &Path,
        options: OpenOptions,
    ) -> Result<(
        Self,
        NotificationReceiver,
        mpsc::Receiver<Trigger>,
        mpsc::Sender<Notification>,
    )> {
        std::fs::create_dir_all(data_dir).context("Failed to create the data directory")?;
        let moved_old_data = move_aside_old_data(data_dir)?;
        let pool = open_database(&data_dir.join("database.db"))?;
        let device = sync::load_or_create_device(data_dir)?;
        let history = History::open(
            &data_dir.join("repository"),
            Index::new(pool),
            device.device_uuid,
            &data_dir.join("untracked_games.json"),
        )?;
        // Before the watcher starts. What can't be put back yet is retried
        // before each capture.
        let recovered_restores =
            store::with_lock(history.root(), || restore::recover(&data_dir.join("txn")))
                .context("Failed to settle an interrupted restore")?
                .rolled_back;
        let (sync_trigger, triggers) = mpsc::channel();
        let inner = Arc::new(Inner {
            history,
            suppression: Suppression::default(),
            txn_root: data_dir.join("txn"),
            sync_trigger,
            checkpoints: OnceLock::new(),
        });
        let (tx, rx) = mpsc::channel();
        let core = Self {
            inner,
            watcher: Mutex::new(None),
            sync_loop: Mutex::new(None),
            running: Mutex::new(None),
            running_games: RunningGames::default(),
            data_dir: data_dir.to_path_buf(),
            device: RwLock::new(device),
            sync_lock: Mutex::new(()),
            last_installs: Mutex::new(HashMap::new()),
            scanned_installs: Mutex::new(HashMap::new()),
            last_prune: Mutex::new(None),
            moved_old_data,
            recovered_restores,
        };
        if options.watch {
            let mut settings = core.get_monitoring_settings()?;
            if let Some(seconds) = options.debounce_seconds {
                settings.snapshot_debounce_seconds = seconds;
            }
            let target: Arc<dyn CaptureTarget> = core.inner.clone();
            let watcher = Watcher::start(
                target,
                settings,
                core.inner.suppression.clone(),
                core.running_games.clone(),
                tx.clone(),
            );
            let _ = core.inner.checkpoints.set(watcher.checkpoints());
            *core.watcher.lock().unwrap_or_else(|e| e.into_inner()) = Some(watcher);
            let index = core.history().index.clone();
            *core.running.lock().unwrap_or_else(|e| e.into_inner()) = Some(RunningMonitor::start(
                Box::new(move || {
                    Ok(index
                        .install_dirs()?
                        .into_iter()
                        .filter_map(|(id, _, dir)| Some((id, dir?)))
                        .collect())
                }),
                core.running_games.clone(),
                tx.clone(),
            ));
        }
        Ok((core, rx, triggers, tx))
    }

    /// Where pre-Fase-III data was moved on this start, if any.
    pub fn moved_old_data(&self) -> Option<&Path> {
        self.moved_old_data.as_deref()
    }

    /// How many writes to live saves a crash interrupted, rolled back on this start.
    pub fn recovered_restores(&self) -> usize {
        self.recovered_restores
    }

    fn txn_root(&self) -> &Path {
        &self.inner.txn_root
    }

    fn history(&self) -> &History {
        &self.inner.history
    }

    pub fn repository_root(&self) -> &Path {
        self.history().root()
    }

    fn reload_watcher(&self) {
        if let Some(watcher) = self
            .watcher
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            watcher.reload();
        }
    }

    /// Captures pending changes, pushes them and stops the background
    /// threads. Also runs on drop.
    pub fn shutdown(&self) {
        // Dropping the watcher captures what is pending, queueing pushes;
        // the loop drains them, pushes once and exits.
        let watcher = self
            .watcher
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        drop(watcher);
        let sync_loop = self
            .sync_loop
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(sync_loop) = sync_loop {
            sync_loop.stop();
        }
        let running = self
            .running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        drop(running);
    }

    /// The games running on this device as of the last check; empty unless
    /// opened with `watch`. Starts and stops are also notified.
    pub fn running_games(&self) -> Vec<i32> {
        self.running_games.ids()
    }

    /// Whether screenshots can be taken on this device (Windows 11 or later).
    /// Front ends hide the screenshot settings when they can't.
    pub fn screenshots_supported() -> bool {
        crate::screenshot::supported()
    }

    /// The version's screenshot, when it has one.
    pub fn screenshot_path(&self, version_id: i32) -> Result<Option<PathBuf>> {
        self.history().screenshot_path(version_id)
    }

    // ---- games, slots, versions ------------------------------------------------

    pub fn get_games(&self) -> Result<Vec<FullGame>> {
        self.history().index.get_games()
    }

    pub fn get_game(&self, id: i32) -> Result<FullGame> {
        self.history().index.get_game(id)
    }

    pub fn get_slot(&self, id: i32) -> Result<FullSlot> {
        self.history().index.get_slot(id)
    }

    pub fn get_version(&self, id: i32) -> Result<FullVersion> {
        self.history().index.get_version(id)
    }

    /// Creates a game (capturing its current saves as the first version) or
    /// edits one. `dirs`, when given, becomes the set of tracked directories.
    pub fn save_game(&self, form: GameForm, dirs: Option<Vec<String>>) -> Result<Game> {
        let game = match form.id {
            None => {
                let game = self
                    .history()
                    .create_game(&form.title, dirs.as_deref().unwrap_or_default())?;
                if !game.tracked_dirs.is_empty() {
                    store::with_lock(self.repository_root(), || {
                        self.inner.checkpoint(&game, VersionReason::Initial)
                    })?;
                }
                game
            }
            Some(id) => self
                .history()
                .update_game(id, &form.title, dirs.as_deref())?,
        };
        self.reload_watcher();
        // A game added from the scan, or renamed to an installed title, gets its install dir.
        self.refresh_install_dirs()?;
        Ok(self.get_game(game.game.id)?.game)
    }

    /// Deletes a game everywhere: the tombstone reaches every device.
    pub fn delete_game(&self, id: i32) -> Result<()> {
        self.history().delete_game(id)?;
        self.reload_watcher();
        Ok(())
    }

    /// Stops tracking a game on this device only (ADR 0006). Pending changes
    /// are captured and published first; the history stays in the local
    /// store and the sync folder, and `adopt_remote_game` brings it back.
    /// Only a game already in the sync folder can be untracked: otherwise
    /// nothing could bring it back.
    pub fn untrack_game(&self, id: i32) -> Result<()> {
        let game = self.get_game(id)?;
        let uuid = game_uuid(&game.game)?;
        let remote = self.remote()?.with_context(|| {
            format!(
                "'{}' can only be removed from this device when sync is on",
                game.game.title
            )
        })?;
        let _guard = self.sync_lock.lock().unwrap_or_else(|e| e.into_inner());
        if remote.store.game(uuid)?.is_none() {
            bail!(
                "'{}' is not in the sync folder yet; sync before removing it from this device",
                game.game.title
            );
        }
        self.exclusive(|| {
            if !game.tracked_dirs.is_empty() {
                self.inner
                    .checkpoint(&self.get_game(id)?, VersionReason::Watcher)
                    .context("Failed to checkpoint the current saves first")?;
            }
            self.history().set_untracked(uuid, true)?;
            self.history().refresh_game(uuid)
        })?;
        // Publishes the captured progress, and pointers without the game.
        for problem in self.push_locked(&remote)? {
            eprintln!("Sync: {problem}");
        }
        Ok(())
    }

    pub fn tag_version(&self, id: i32, tag: String) -> Result<Version> {
        self.history().tag_version(id, Some(tag))
    }

    /// Checkpoints the live saves now (a no-op when unchanged).
    pub fn checkpoint(&self, game_id: i32) -> Result<CheckpointOutcome> {
        store::with_lock(self.repository_root(), || {
            let game = self.get_game(game_id)?;
            self.inner.checkpoint(&game, VersionReason::Manual)
        })
    }

    /// Runs a write to live saves with capture suppressed, under the
    /// repository lock, after checkpointing the live state, so uncaptured
    /// progress is never overwritten. `restoring` is the version `apply` will
    /// restore, if any: the checkpoint then also covers the retired tracked
    /// directories that restore writes back. Refuses while the game is seen
    /// running, which could save over the result; when that's unknown, the
    /// transaction in `restore::apply` still keeps the saves whole.
    fn guarded<T>(
        &self,
        game_id: i32,
        reason: VersionReason,
        restoring: Option<&FullVersion>,
        apply: impl FnOnce(&FullGame) -> Result<T>,
    ) -> Result<T> {
        self.exclusive(|| {
            let game = self.covering(self.get_game(game_id)?, restoring)?;
            if sync::game_is_running(game.game.install_dir.as_deref()) == Some(true) {
                return Err(anyhow::Error::new(ErrorKind::GameRunning)
                    .context("Nothing was changed. Close the game and try again"));
            }
            if !game.tracked_dirs.is_empty() {
                self.inner
                    .checkpoint(&game, reason)
                    .context("Failed to checkpoint the current saves first")?;
            }
            apply(&self.get_game(game_id)?)
        })
    }

    /// Runs `apply` with capture suppressed and the repository lock held.
    /// `guarded` adds the checkpoint; only callers that must pull before they
    /// know what to capture (adoption, a remotely deleted slot) use it directly.
    fn exclusive<T>(&self, apply: impl FnOnce() -> Result<T>) -> Result<T> {
        let suppressed = self.inner.suppression.hold();
        let result = store::with_lock(self.repository_root(), apply);
        drop(suppressed);
        self.reload_watcher();
        result
    }

    /// `game` plus the retired tracked directories restoring `version` writes back.
    fn covering(&self, mut game: FullGame, version: Option<&FullVersion>) -> Result<FullGame> {
        let Some(version) = version else {
            return Ok(game);
        };
        let restored: HashSet<Uuid> = self
            .history()
            .version_meta(version)?
            .files
            .iter()
            .map(|file| file.tracked_dir_uuid)
            .collect();
        game.tracked_dirs.extend(
            self.history()
                .index
                .retired_tracked_dirs(game.game.id)?
                .into_iter()
                .filter(|dir| {
                    Uuid::parse_str(&dir.uuid).is_ok_and(|uuid| restored.contains(&uuid))
                }),
        );
        Ok(game)
    }

    /// The version activating `slot_id` restores, if it has one.
    fn slot_target(&self, game: &FullGame, slot_id: i32) -> Result<Option<FullVersion>> {
        game.slots
            .iter()
            .find(|slot| slot.id == slot_id)
            .and_then(|slot| slot.active_version_id)
            .map(|id| self.get_version(id))
            .transpose()
    }

    /// Writes `version` into the live directories and makes it (and its slot) active.
    fn restore_locked(&self, game: &FullGame, version: &FullVersion) -> Result<()> {
        let history = self.history();
        let record = history.record(&game.game)?;
        let meta = history.version_meta(version)?;
        let prepared = restore::prepare(
            &history.store,
            &record,
            &game.tracked_dirs,
            &meta,
            version.version.retired,
        )?;
        restore::apply(&history.store, prepared, self.txn_root())?;
        history
            .index
            .set_active_version(version.slot.id, Some(version.version.id))?;
        history.index.set_active_slot(game.game.id, version.slot.id)
    }

    pub fn restore_version(&self, version_id: i32) -> Result<()> {
        let version = self.get_version(version_id)?;
        self.guarded(
            version.game.id,
            VersionReason::PreRestore,
            Some(&version),
            |game| self.restore_locked(game, &version),
        )
    }

    /// Applies another device's progress: the same as a restore, but only
    /// while the game has an active slot here.
    pub fn apply_remote_version(&self, version_id: i32) -> Result<()> {
        let version = self.get_version(version_id)?;
        if self.get_game(version.game.id)?.active_slot.is_none() {
            bail!("Cannot apply remote progress without an active local slot");
        }
        self.restore_version(version_id)
    }

    pub fn create_slot(&self, game_id: i32, name: &str) -> Result<FullGame> {
        self.guarded(game_id, VersionReason::PreSlotCreate, None, |game| {
            let slot = self.history().create_slot(game_id, name)?;
            let prepared = restore::clear_only(&game.tracked_dirs)?;
            restore::apply(&self.history().store, prepared, self.txn_root())?;
            self.history().index.set_active_slot(game_id, slot.id)?;
            let game = self.get_game(game_id)?;
            if !game.tracked_dirs.is_empty() {
                self.inner.checkpoint(&game, VersionReason::Initial)?;
            }
            self.get_game(game_id)
        })
    }

    pub fn switch_slot(&self, game_id: i32, slot_id: i32) -> Result<FullGame> {
        let game = self.get_game(game_id)?;
        if game
            .active_slot
            .as_ref()
            .is_some_and(|slot| slot.id == slot_id)
        {
            return self.get_game(game_id);
        }
        let target = self.slot_target(&game, slot_id)?;
        self.guarded(game_id, VersionReason::PreSwitch, target.as_ref(), |game| {
            self.activate_slot(game, slot_id)?;
            self.get_game(game_id)
        })
    }

    fn activate_slot(&self, game: &FullGame, slot_id: i32) -> Result<()> {
        let slot = game
            .slots
            .iter()
            .find(|slot| slot.id == slot_id)
            .context("The slot does not belong to this game")?;
        match slot.active_version_id {
            Some(version_id) => self.restore_locked(game, &self.get_version(version_id)?),
            None => {
                restore::apply(
                    &self.history().store,
                    restore::clear_only(&game.tracked_dirs)?,
                    self.txn_root(),
                )?;
                self.history().index.set_active_slot(game.game.id, slot_id)
            }
        }
    }

    pub fn rename_slot(&self, slot_id: i32, name: &str) -> Result<FullGame> {
        let slot = self.history().rename_slot(slot_id, name)?;
        self.get_game(slot.game_id)
    }

    /// Deletes a slot. The last slot can't be deleted, and deleting the active
    /// one first switches to `replacement_slot_id`.
    pub fn delete_slot(&self, slot_id: i32, replacement_slot_id: Option<i32>) -> Result<FullGame> {
        let slot = self.get_slot(slot_id)?.slot;
        let game = self.get_game(slot.game_id)?;
        if game.slots.len() <= 1 {
            bail!(
                "Cannot delete '{}': a game must keep at least one slot",
                slot.name
            );
        }
        if game
            .active_slot
            .as_ref()
            .is_some_and(|active| active.id == slot_id)
        {
            let replacement = replacement_slot_id.context(
                "Deleting the active slot requires a replacement slot to switch to first",
            )?;
            if replacement == slot_id {
                bail!("The replacement slot must not be the slot being deleted");
            }
            if !game
                .slots
                .iter()
                .any(|candidate| candidate.id == replacement)
            {
                bail!("Replacement slot {replacement} does not belong to this game");
            }
            let target = self.slot_target(&game, replacement)?;
            self.guarded(
                game.game.id,
                VersionReason::PreSwitch,
                target.as_ref(),
                |game| self.activate_slot(game, replacement),
            )?;
        }
        self.history().delete_slot(slot_id)?;
        self.get_game(game.game.id)
    }

    /// Stores an image (or an executable's icon) as the game's shared icon
    /// and returns its local path.
    pub fn set_game_icon(&self, game_id: i32, source_path: &str) -> Result<String> {
        let source = Path::new(source_path);
        let is_exe = source
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"));
        let image = if is_exe {
            let cache = self.data_dir.join("cache");
            std::fs::create_dir_all(&cache).context("Failed to create the cache directory")?;
            let extracted = cache.join(format!(".icon-{}.png", store::short_random()));
            let image = extract_exe_icon(source, &extracted).and_then(|()| {
                image::open(&extracted).context("Failed to read the extracted icon")
            });
            let _ = std::fs::remove_file(&extracted);
            image?
        } else {
            image::open(source).context("Failed to open image file")?
        };
        let png = crate::icon::normalize(&image)?;
        let game = store::with_lock(self.repository_root(), || {
            self.history().set_icon(game_id, &png)
        })?;
        game.game.icon_path.context("The icon was not stored")
    }

    /// `Some(hue)` (0–359) sets the game's accent color; `None` returns to
    /// the color taken from its icon.
    pub fn set_game_accent(&self, game_id: i32, hue: Option<u16>) -> Result<Game> {
        store::with_lock(self.repository_root(), || {
            self.history().set_accent(game_id, hue)
        })
        .map(|game| game.game)
    }

    /// Rebuilds a version's files under `destination` without touching live
    /// saves: one folder per tracked directory.
    pub fn extract_version(&self, version_id: i32, destination: &Path) -> Result<usize> {
        let version = self.get_version(version_id)?;
        if version.version.retired {
            bail!("This checkpoint was retired and can no longer be extracted");
        }
        let history = self.history();
        let record = history.record(&version.game)?;
        let meta = history.version_meta(&version)?;
        for file in &meta.files {
            let folder = record
                .tracked_dirs
                .iter()
                .find(|dir| dir.uuid == file.tracked_dir_uuid)
                .map(|dir| dir.folder_slug.clone())
                .unwrap_or_else(|| file.tracked_dir_uuid.to_string());
            let target = destination.join(folder).join(&file.path);
            std::fs::create_dir_all(target.parent().context("invalid target")?)
                .with_context(|| format!("Failed to create the folder for {}", target.display()))?;
            let mut out = std::fs::File::create(&target)
                .with_context(|| format!("Failed to create {}", target.display()))?;
            crate::chunks::write_file(&history.store, file, &mut out)?;
        }
        Ok(meta.files.len())
    }

    /// Holds the repository lock: a failed read quarantines the chunk, which
    /// must not race a capture republishing it.
    pub fn check_integrity(&self, deep: bool) -> Result<crate::integrity::IntegrityReport> {
        store::with_lock(self.repository_root(), || {
            crate::integrity::check(&self.history().store, deep)
        })
    }

    /// Rebuilds the index from the repository files.
    pub fn reindex(&self) -> Result<()> {
        self.history().reindex()?;
        self.reload_watcher();
        Ok(())
    }

    // ---- export / import ---------------------------------------------------------

    pub fn inspect_history_bundle(path: &Path) -> Result<HistoryBundleSummary> {
        crate::bundle::inspect(path)
    }

    pub fn export_game_history(
        &self,
        game_id: i32,
        destination: &Path,
    ) -> Result<HistoryTransferReport> {
        store::with_lock(self.repository_root(), || {
            crate::bundle::export(self.history(), game_id, destination)
        })
    }

    pub fn import_game_history(
        &self,
        bundle_path: &Path,
        mappings: &HashMap<Uuid, PathBuf>,
    ) -> Result<HistoryTransferReport> {
        let report = store::with_lock(self.repository_root(), || {
            crate::bundle::import(self.history(), bundle_path, mappings)
        })?;
        // Importing an untracked game's history tracks it again.
        if self.history().untracked()?.contains(&report.game_uuid) {
            self.history().set_untracked(report.game_uuid, false)?;
            self.history().refresh_game(report.game_uuid)?;
        }
        self.reload_watcher();
        Ok(report)
    }

    // ---- settings ------------------------------------------------------------------

    pub fn get_monitoring_settings(&self) -> Result<MonitoringSettings> {
        let path = self.data_dir.join("monitoring_settings.json");
        Ok(read_json(&path).unwrap_or_default())
    }

    pub fn update_monitoring_settings(
        &self,
        settings: MonitoringSettings,
    ) -> Result<MonitoringSettings> {
        if settings.snapshot_debounce_seconds < 1.0 || settings.version_cooldown_minutes < 1 {
            bail!("The debounce must be at least 1 second and the cooldown at least 1 minute");
        }
        write_json(&self.data_dir.join("monitoring_settings.json"), &settings)?;
        if let Some(watcher) = self
            .watcher
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            watcher.update_settings(settings);
        }
        Ok(settings)
    }

    pub fn get_scan_roots(&self) -> Result<Vec<String>> {
        self.history().index.get_scan_roots()
    }

    pub fn add_scan_root(&self, path: &str) -> Result<()> {
        self.history().index.add_scan_root(path)
    }

    pub fn remove_scan_root(&self, path: &str) -> Result<()> {
        self.history().index.remove_scan_root(path)
    }

    /// Installed games with saves to track. Also refreshes the install dirs
    /// of the games already tracked here.
    pub fn scan_installed(&self) -> Result<Vec<InstallScannerItem>> {
        let cache_dir = self.data_dir.join("cache");
        let candidates =
            crate::install_scanner::scan_installed_candidates(&cache_dir, &self.get_scan_roots()?)?;
        self.remember_installs(
            candidates
                .iter()
                .map(|item| {
                    (
                        store::normalize_display_name(&item.title),
                        item.install_path.clone(),
                    )
                })
                .collect(),
        )?;
        let mut items: Vec<InstallScannerItem> = candidates
            .into_iter()
            .filter(|item| !item.dirs.is_empty())
            .collect();
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
            if !cache_png.exists() {
                let source = Path::new(source_path);
                let is_exe = source
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"));
                let extracted = if is_exe {
                    extract_exe_icon(source, &cache_png).is_ok()
                } else {
                    image::open(source)
                        .and_then(|img| img.save(&cache_png))
                        .is_ok()
                };
                if !extracted {
                    item.icon_path = None;
                    continue;
                }
            }
            item.icon_path = Some(cache_png.to_string_lossy().into_owned());
        }
        Ok(items)
    }

    /// Keeps `installs` (normalized title → install dir) as the last scan's
    /// result and refreshes install dirs from it.
    fn remember_installs(&self, installs: HashMap<String, Option<String>>) -> Result<()> {
        *self
            .scanned_installs
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = installs;
        self.refresh_install_dirs()
    }

    /// Install dirs are device-local and come from the install scan, matched
    /// by title, so games added by hand have none until a scan finds them.
    /// A game the last scan found gets the dir it found, so a moved game
    /// follows. A stored dir that no longer exists is cleared (uninstalled);
    /// one that still exists is kept even when the scan missed the game.
    fn refresh_install_dirs(&self) -> Result<()> {
        let scanned = self
            .scanned_installs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let index = &self.history().index;
        for (id, title, stored) in index.install_dirs()? {
            let wanted = match scanned.get(&title).cloned().flatten() {
                Some(dir) => Some(dir),
                None => stored.clone().filter(|dir| Path::new(dir).is_dir()),
            };
            if wanted != stored {
                index.set_install_dir(id, wanted.as_deref())?;
            }
        }
        Ok(())
    }

    // ---- devices and sync ------------------------------------------------------------

    pub fn device_metadata(&self) -> DeviceMetadata {
        self.device
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Sets this device's name and kind together; both are published with it.
    pub fn update_device(&self, name: &str, kind: Option<DeviceKind>) -> Result<DeviceMetadata> {
        let name = name.trim();
        if name.is_empty() {
            bail!("Device name cannot be empty");
        }
        let mut device = self.device.write().unwrap_or_else(|e| e.into_inner());
        device.name = name.to_string();
        device.kind = kind;
        sync::save_device(&self.data_dir, &device)?;
        Ok(device.clone())
    }

    pub fn get_sync_settings(&self) -> Result<SyncSettings> {
        sync::load_settings(&self.data_dir.join("sync_settings.json"))
    }

    pub fn update_sync_settings(&self, settings: SyncSettings) -> Result<SyncSettings> {
        let _guard = self.sync_lock.lock().unwrap_or_else(|e| e.into_inner());
        let settings = SyncSettings {
            remote_path: settings.remote_path.map(|p| p.trim().to_string()),
        };
        if let Some(path) = settings.remote_path.as_deref() {
            if path.is_empty() {
                bail!("Remote path cannot be empty");
            }
            Remote::open(Path::new(path), &self.data_dir)?;
        }
        sync::save_settings(&self.data_dir.join("sync_settings.json"), &settings)?;
        Ok(settings)
    }

    fn remote(&self) -> Result<Option<Remote>> {
        self.get_sync_settings()?
            .remote_path
            .map(|path| Remote::open(Path::new(&path), &self.data_dir))
            .transpose()
    }

    fn require_remote(&self) -> Result<Remote> {
        self.remote()?.context("Sync remote is not configured")
    }

    pub fn remove_sync_device(&self, device_uuid: Uuid) -> Result<()> {
        if device_uuid == self.device_metadata().device_uuid {
            bail!("The current device cannot remove itself");
        }
        self.require_remote()?.remove_device(device_uuid)
    }

    /// Runs a full sync when a remote is configured.
    pub fn sync_if_configured(&self) -> Result<Option<SyncReport>> {
        if self.get_sync_settings()?.remote_path.is_none() {
            return Ok(None);
        }
        self.refresh_sync().map(Some)
    }

    /// Publishes local history and pointers (after a new version).
    pub fn push_sync(&self) -> Result<()> {
        let Some(remote) = self.remote()? else {
            return Ok(());
        };
        let _guard = self.sync_lock.lock().unwrap_or_else(|e| e.into_inner());
        let problems = self.push_locked(&remote)?;
        if !problems.is_empty() {
            bail!("Some games could not be pushed: {}", problems.join("; "));
        }
        Ok(())
    }

    /// Publishes every game (one game's failure doesn't stop the others) and
    /// this device's pointers. Returns the per-game problems.
    fn push_locked(&self, remote: &Remote) -> Result<Vec<String>> {
        let history = self.history();
        let state_path = self.data_dir.join("sync_state.json");
        let mut state: SyncState = read_json(&state_path).unwrap_or_default();
        let now = Utc::now();
        let full = state
            .last_full_heal
            .is_none_or(|at| now - at > chrono::Duration::days(1));
        let pulled: HashSet<Uuid> = state
            .recent_pulls
            .iter()
            .filter(|(_, at)| now - **at <= heal_window())
            .map(|(uuid, _)| *uuid)
            .collect();
        let heal = if full {
            Heal::All
        } else {
            Heal::Recent {
                since: now - heal_window(),
                pulled: &pulled,
            }
        };
        // Remote markers too: healing must not re-upload what another device
        // retired before this one pulled the retirement.
        let mut markers = remote.store.markers()?;
        markers.extend(history.store.markers()?);
        let mut transfer = Transfer::new(&history.store, &remote.store, Completeness::Partial)
            .screenshots(self.get_monitoring_settings()?.sync_screenshots);
        let mut problems = Vec::new();
        for game in history.store.catalog()? {
            if let Err(error) = transfer.copy_game(&game, &markers, heal) {
                problems.push(format!("{}: {error:#}", game.meta.title));
            }
        }
        let device = self.device_metadata();
        let pointers = DeviceState::from_pointers(device.device_uuid, &history.index.pointers()?);
        let published = remote.publish(&device, &pointers)?;
        sync::save_device(&self.data_dir, &published)?;
        *self.device.write().unwrap_or_else(|e| e.into_inner()) = published;
        if full && problems.is_empty() {
            state.last_full_heal = Some(now);
            write_json(&state_path, &state)?;
        }
        Ok(problems)
    }

    fn pull_game(&self, remote: &Remote, game: &GameRecord) -> Result<Vec<Uuid>> {
        let history = self.history();
        let markers = remote.store.markers()?;
        let screenshots = self.get_monitoring_settings()?.sync_screenshots;
        let copy = || {
            let mut transfer = Transfer::new(&remote.store, &history.store, Completeness::Partial)
                .screenshots(screenshots);
            transfer.copy_game(game, &markers, Heal::None)?;
            Ok(transfer.report)
        };
        let deleted_active = match history.index.game_id(game.meta.uuid)? {
            Some(id) => {
                let local = self.get_game(id)?;
                let active = local.active_slot.as_ref().map(|slot| slot.uuid.clone());
                game.slots
                    .iter()
                    .any(|slot| {
                        slot.meta.deleted_at.is_some() && active == Some(slot.meta.uuid.to_string())
                    })
                    .then_some(id)
            }
            None => None,
        };
        let report = match deleted_active {
            Some(id) => self.exclusive(|| {
                let before = self.get_game(id)?;
                let report = copy()?;
                self.record_pulls(&report.copied_versions)?;
                self.leave_deleted_slot(&before)?;
                Ok(report)
            })?,
            None => {
                let report = store::with_lock(history.root(), copy)?;
                self.record_pulls(&report.copied_versions)?;
                history.refresh_game(game.meta.uuid)?;
                report
            }
        };
        Ok(report.not_arrived)
    }

    /// Indexes a pulled game whose slot active here (in `before`, read before
    /// the pull) another device may have deleted. The slot's fallback is
    /// loaded only when the deletion shows that device saw everything here:
    /// every version of the slot, and live saves equal to the active one.
    /// Otherwise the unseen versions and the live saves move to a new slot
    /// of the same name, left live. The caller holds the lock with capture
    /// suppressed.
    fn leave_deleted_slot(&self, before: &FullGame) -> Result<()> {
        let history = self.history();
        let uuid = crate::history::game_uuid(&before.game)?;
        let record = history
            .store
            .game(uuid)?
            .context("Pulled game is missing")?;
        let deleted = before.active_slot.as_ref().and_then(|active| {
            record
                .slots
                .into_iter()
                .find(|slot| slot.meta.uuid.to_string() == active.uuid)
                .filter(|slot| slot.meta.deleted_at.is_some())
        });
        history.refresh_game(uuid)?;
        let (Some(deleted), Some(id)) = (deleted, history.index.game_id(uuid)?) else {
            return Ok(());
        };
        let game = self.get_game(id)?;
        let fallback = game.active_slot.clone();
        let target = match &fallback {
            Some(slot) => self.slot_target(&game, slot.id)?,
            None => None,
        };
        let live = self.covering(game.clone(), target.as_ref())?;
        let files = history.capture(&live)?;
        let dirs: HashSet<Uuid> = live
            .tracked_dirs
            .iter()
            .filter_map(|dir| Uuid::parse_str(&dir.uuid).ok())
            .collect();
        let active = before.last_version.as_ref().map(|version| &version.uuid);
        let known: Vec<_> = deleted
            .versions
            .iter()
            .find(|version| Some(&version.uuid.to_string()) == active)
            .map(|meta| meta.files.clone())
            .unwrap_or_default()
            .into_iter()
            .filter(|file| dirs.contains(&file.tracked_dir_uuid))
            .collect();
        let parents: HashMap<Uuid, Option<Uuid>> = deleted
            .versions
            .iter()
            .map(|version| (version.uuid, version.parent_uuid))
            .collect();
        let retired = history.retired()?;
        let mut unseen: Vec<_> = deleted
            .versions
            .into_iter()
            .filter(|version| {
                !deleted.meta.seen_versions.contains(&version.uuid)
                    && !retired.contains(&version.uuid)
            })
            .collect();
        if let Some(fallback) = fallback
            && unseen.is_empty()
            && files == known
        {
            return self.activate_slot(&game, fallback.id);
        }
        let name = &deleted.meta.name;
        let slot = history
            .create_slot(id, name)
            .or_else(|_| history.create_slot(id, &format!("{name} (kept)")))?;
        history.index.set_active_slot(id, slot.id)?;
        // Copies keep their branches: each hangs off the copy of its nearest
        // copied ancestor; seen and retired ancestors stay behind.
        let kept = self.get_game(id)?;
        unseen.sort_by_key(|version| (version.created_at, version.version_number));
        let copied: HashSet<Uuid> = unseen.iter().map(|version| version.uuid).collect();
        let nearest = |mut parent: Option<Uuid>| {
            for _ in 0..parents.len() {
                match parent {
                    Some(uuid) if !copied.contains(&uuid) => {
                        parent = parents.get(&uuid).copied().flatten()
                    }
                    _ => break,
                }
            }
            parent.filter(|uuid| copied.contains(uuid))
        };
        let mut pending: Vec<_> = unseen
            .into_iter()
            .map(|version| (nearest(version.parent_uuid), version))
            .collect();
        let mut copies: HashMap<Uuid, Version> = HashMap::new();
        while !pending.is_empty() {
            // Parent order, not clock order; a cycle's first member is a root.
            let ready = pending
                .iter()
                .position(|(parent, _)| parent.is_none_or(|uuid| copies.contains_key(&uuid)))
                .unwrap_or(0);
            let (parent, version) = pending.remove(ready);
            let parent = parent
                .and_then(|uuid| copies.get(&uuid))
                .map(|copy| Uuid::parse_str(&copy.uuid))
                .transpose()?;
            let copy = history.commit_version(
                &kept,
                &slot,
                parent,
                version.reason,
                version.files,
                version.created_at,
            )?;
            if version.tag.is_some() {
                history.tag_version(copy.id, version.tag)?;
            }
            copies.insert(version.uuid, copy);
        }
        // A live branch the deleter saw starts fresh rather than under
        // whichever copy came last.
        let active = active.and_then(|uuid| copies.get(&Uuid::parse_str(uuid).ok()?));
        history
            .index
            .set_active_version(slot.id, active.map(|copy| copy.id))?;
        let kept = FullGame {
            tracked_dirs: live.tracked_dirs,
            ..self.get_game(id)?
        };
        let outcome = self.inner.checkpoint(&kept, VersionReason::Initial)?;
        // Emptied saves still get a version: a slot with history but no
        // active version is not monitored.
        if matches!(outcome, CheckpointOutcome::Empty) && !copies.is_empty() {
            let now = Utc::now();
            history.commit_version(&kept, &slot, None, VersionReason::Initial, Vec::new(), now)?;
        }
        Ok(())
    }

    /// Remembers pulled versions for push healing and forgets old entries.
    /// Callers hold `sync_lock`.
    fn record_pulls(&self, copied: &[Uuid]) -> Result<()> {
        if copied.is_empty() {
            return Ok(());
        }
        let state_path = self.data_dir.join("sync_state.json");
        let mut state: SyncState = read_json(&state_path).unwrap_or_default();
        let now = Utc::now();
        state
            .recent_pulls
            .retain(|_, at| now - *at <= heal_window());
        state
            .recent_pulls
            .extend(copied.iter().map(|uuid| (*uuid, now)));
        write_json(&state_path, &state)
    }

    /// Every game this device has history for, tombstones included, except
    /// untracked games: those are remote games not on this device.
    fn local_game_uuids(&self, untracked: &HashSet<Uuid>) -> Result<HashSet<Uuid>> {
        Ok(self
            .history()
            .store
            .game_metas()?
            .iter()
            .map(|g| g.uuid)
            .filter(|uuid| !untracked.contains(uuid))
            .collect())
    }

    fn pull_locked(
        &self,
        remote: &Remote,
        remote_games: &[GameRecord],
        local_uuids: &HashSet<Uuid>,
    ) -> Vec<String> {
        let mut problems = Vec::new();
        for remote_game in remote_games
            .iter()
            .filter(|g| local_uuids.contains(&g.meta.uuid))
        {
            match self.pull_game(remote, remote_game) {
                Ok(not_arrived) if !not_arrived.is_empty() => problems.push(format!(
                    "{}: {} version(s) have not fully arrived yet",
                    remote_game.meta.title,
                    not_arrived.len()
                )),
                Ok(_) => {}
                Err(error) => problems.push(format!("{}: {error:#}", remote_game.meta.title)),
            }
        }
        problems
    }

    /// Pull, adopt, fast-forward, push, report.
    pub fn refresh_sync(&self) -> Result<SyncReport> {
        let remote = self.require_remote()?;
        let _guard = self.sync_lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut problems = Vec::new();
        let remote_games = remote.store.catalog()?;
        // Only an explicit adoption brings an untracked game back.
        let untracked = self.history().untracked()?;

        // Link games tracked on both devices before sync was enabled.
        for remote_game in remote_games.iter().filter(|g| g.meta.deleted_at.is_none()) {
            if untracked.contains(&remote_game.meta.uuid)
                || self
                    .history()
                    .index
                    .game_id(remote_game.meta.uuid)?
                    .is_some()
            {
                continue;
            }
            match self.match_local_game(remote_game)? {
                LocalMatch::None => {}
                LocalMatch::Same(local) => {
                    if let Err(error) = self.link_game(&local, remote_game) {
                        problems.push(format!("{}: {error:#}", remote_game.meta.title));
                    }
                }
                LocalMatch::Collision(reason) => {
                    problems.push(format!("{}: {reason}", remote_game.meta.title))
                }
            }
        }

        let local_uuids = self.local_game_uuids(&untracked)?;
        problems.extend(self.pull_locked(&remote, &remote_games, &local_uuids));

        // Adopt remote games installed here. Scanning is slow, so it runs
        // only once something needs it: adoption, or a fast-forward that
        // must know whether the game is running.
        let unknown: Vec<&GameRecord> = remote_games
            .iter()
            .filter(|g| g.meta.deleted_at.is_none() && !local_uuids.contains(&g.meta.uuid))
            .collect();
        let scan_roots = self.get_scan_roots()?;
        let installs = std::cell::OnceCell::new();
        let installed_titles = || -> &HashMap<String, Option<String>> {
            installs.get_or_init(|| {
                crate::install_scanner::scan_installed_candidates(
                    &self.data_dir.join("cache"),
                    &scan_roots,
                )
                .unwrap_or_default()
                .into_iter()
                .map(|item| {
                    (
                        store::normalize_display_name(&item.title),
                        item.install_path,
                    )
                })
                .collect()
            })
        };
        let mut auto_applied = HashSet::new();
        for remote_game in unknown {
            if !installed_titles()
                .contains_key(&store::normalize_display_name(&remote_game.meta.title))
            {
                continue;
            }
            // Checked after the scan, so the report still says it's installed.
            if untracked.contains(&remote_game.meta.uuid) {
                continue;
            }
            if !matches!(self.match_local_game(remote_game)?, LocalMatch::None) {
                continue;
            }
            match self.adopt(&remote, remote_game) {
                Ok(true) => {
                    auto_applied.insert(remote_game.meta.uuid);
                }
                Ok(false) => {}
                Err(error) => problems.push(format!("{}: {error:#}", remote_game.meta.title)),
            }
        }

        // Fast-forward slots other devices moved ahead.
        let remote_games = remote.store.catalog()?;
        for game in self.get_games()? {
            let Some(remote_game) = remote_games
                .iter()
                .find(|g| g.meta.uuid.to_string() == game.game.uuid)
            else {
                continue;
            };
            let install = || {
                installed_titles()
                    .get(&game.game.title_normalized)
                    .cloned()
                    .flatten()
            };
            match self.fast_forward(&remote, remote_game, &game, install) {
                Ok(true) => {
                    auto_applied.insert(remote_game.meta.uuid);
                }
                Ok(false) => {}
                Err(error) => problems.push(format!("{}: {error:#}", game.game.title)),
            }
        }

        problems.extend(self.push_locked(&remote)?);
        self.reload_watcher();
        let remote = self.require_remote()?;
        // Adopted games get their install dir; uninstalled ones lose it.
        let refreshed = match installs.get() {
            Some(scanned) => self.remember_installs(scanned.clone()),
            None => self.refresh_install_dirs(),
        };
        if let Err(error) = refreshed {
            problems.push(format!("Install folders: {error:#}"));
        }
        let installed = installs.get().cloned().unwrap_or_default();
        *self.last_installs.lock().unwrap_or_else(|e| e.into_inner()) = installed.clone();
        self.sync_report(&remote, &auto_applied, &installed, problems)
    }

    /// The sync state as last published, without pulling or pushing.
    pub fn sync_status(&self) -> Result<SyncReport> {
        let remote = self.require_remote()?;
        let installed = self
            .last_installs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        self.sync_report(&remote, &HashSet::new(), &installed, Vec::new())
    }

    /// Adopts the remote game `uuid` on this device.
    pub fn adopt_remote_game(&self, uuid: Uuid) -> Result<FullGame> {
        let remote = self.require_remote()?;
        let _guard = self.sync_lock.lock().unwrap_or_else(|e| e.into_inner());
        let game = remote.store.game(uuid)?.context("Remote game not found")?;
        if game.meta.deleted_at.is_some() {
            bail!("This game was deleted on another device");
        }
        let found = self.match_local_game(&game)?;
        if !matches!(found, LocalMatch::Collision(_)) {
            // Adopting is how an untracked game comes back.
            self.history().set_untracked(uuid, false)?;
        }
        match found {
            LocalMatch::None => {
                self.adopt(&remote, &game)?;
            }
            LocalMatch::Same(local) => {
                self.link_game(&local, &game)?;
                self.pull_game(&remote, &game)?;
            }
            LocalMatch::Collision(reason) => bail!(reason),
        }
        for problem in self.push_locked(&remote)? {
            eprintln!("Sync: {problem}");
        }
        self.reload_watcher();
        self.refresh_install_dirs()?;
        let id = self
            .history()
            .index
            .game_id(uuid)?
            .context("Adopted game is not indexed")?;
        self.get_game(id)
    }

    fn match_local_game(&self, remote_game: &GameRecord) -> Result<LocalMatch> {
        let title = store::normalize_display_name(&remote_game.meta.title);
        let remote_paths: Vec<String> = remote_game
            .tracked_dirs
            .iter()
            .filter(|dir| dir.retired_at.is_none())
            .map(|dir| crate::path_placeholders::expand_path_placeholders(&dir.path))
            .collect();
        let shares_path = |game: &FullGame| {
            game.tracked_dirs.iter().any(|dir| {
                remote_paths
                    .iter()
                    .any(|path| crate::history::same_path(path, &dir.path))
            })
        };
        let games = self.get_games()?;
        if let Some(local) = games.iter().find(|g| g.game.title_normalized == title) {
            return Ok(if shares_path(local) {
                LocalMatch::Same(Box::new(local.clone()))
            } else {
                LocalMatch::Collision(format!(
                    "a different local game is also named '{}'; rename one of them to sync it",
                    local.game.title
                ))
            });
        }
        let overlaps_path = |game: &FullGame| {
            game.tracked_dirs.iter().any(|dir| {
                remote_paths
                    .iter()
                    .any(|path| crate::history::overlapping_paths(path, &dir.path))
            })
        };
        if let Some(local) = games.iter().find(|g| overlaps_path(g)) {
            return Ok(LocalMatch::Collision(format!(
                "its save folder is already tracked by '{}'",
                local.game.title
            )));
        }
        Ok(LocalMatch::None)
    }

    /// Re-keys a local game that was never synced to the remote game it
    /// duplicates, so both histories become slots of one game.
    fn link_game(&self, local: &FullGame, remote_game: &GameRecord) -> Result<()> {
        let history = self.history();
        let old = history.record(&local.game)?;
        let new_uuid = remote_game.meta.uuid;
        let slug = remote_game.meta.folder_slug.clone();
        let dir_map: HashMap<Uuid, Uuid> = old
            .tracked_dirs
            .iter()
            .filter_map(|dir| {
                let path = crate::path_placeholders::expand_path_placeholders(&dir.path);
                remote_game
                    .tracked_dirs
                    .iter()
                    .find(|r| {
                        crate::history::same_path(
                            &crate::path_placeholders::expand_path_placeholders(&r.path),
                            &path,
                        )
                    })
                    .map(|r| (dir.uuid, r.uuid))
            })
            .collect();
        let pointers: Vec<_> = history
            .index
            .pointers()?
            .into_iter()
            .filter(|p| p.game == old.meta.uuid)
            .collect();
        store::with_lock(history.root(), || {
            history.store.write_game(&remote_game.meta)?;
            // The remote icon wins; the local one (e.g. the install
            // scanner's) is kept only when the remote has none (ADR 0005).
            if remote_game.meta.icon_sha256.is_none()
                && let Some(wanted) = &old.meta.icon_sha256
                && let Some(png) = history.store.read_icon(&old.meta.folder_slug)?
                && crate::icon::sha256(&png) == *wanted
            {
                let mut meta = remote_game.meta.clone();
                meta.icon_sha256 = Some(wanted.clone());
                if meta.accent_hue_source == store::AccentHueSource::Auto {
                    meta.accent_hue = old.meta.accent_hue;
                    meta.accent_hue_source = old.meta.accent_hue_source;
                }
                meta.updated_at =
                    Utc::now().max(meta.updated_at + chrono::Duration::milliseconds(1));
                history.store.write_icon(&slug, &png)?;
                history.store.write_game(&meta)?;
            }
            for dir in old
                .tracked_dirs
                .iter()
                .filter(|dir| !dir_map.contains_key(&dir.uuid))
            {
                history.store.write_tracked_dir(
                    &slug,
                    &store::TrackedDirMeta {
                        game_uuid: new_uuid,
                        ..dir.clone()
                    },
                )?;
            }
            for dir in remote_game
                .tracked_dirs
                .iter()
                .filter(|r| dir_map.values().any(|v| *v == r.uuid))
            {
                history.store.write_tracked_dir(&slug, dir)?;
            }
            let markers = history.store.markers()?;
            for slot in &old.slots {
                history.store.write_slot(
                    &slug,
                    &store::SlotMeta {
                        game_uuid: new_uuid,
                        ..slot.meta.clone()
                    },
                )?;
                for version in &slot.versions {
                    let mut version = version.clone();
                    version.game_uuid = new_uuid;
                    for file in &mut version.files {
                        if let Some(mapped) = dir_map.get(&file.tracked_dir_uuid) {
                            file.tracked_dir_uuid = *mapped;
                        }
                    }
                    version.files.sort();
                    history
                        .store
                        .publish_version(&slug, &slot.meta.folder_slug, &version)?;
                    let screenshot = history.store.screenshot_path(
                        &old.meta.folder_slug,
                        &slot.meta.folder_slug,
                        &version.folder_slug,
                    );
                    if screenshot.exists() {
                        history.store.write_screenshot(
                            &slug,
                            &slot.meta.folder_slug,
                            &version.folder_slug,
                            &std::fs::read(&screenshot)?,
                        )?;
                    }
                    if let Some(marker) = markers.get(&version.uuid) {
                        history.store.replace_marker(&store::RetirementMarker {
                            game_uuid: new_uuid,
                            ..marker.clone()
                        })?;
                    }
                }
            }
            // The old identity may already be on the remote: keep a tombstone
            // so no device resurrects it, and drop its now-duplicated history.
            history.store.drop_game_history(&old.meta.folder_slug)?;
            let now = Utc::now();
            history.store.write_game(&store::GameMeta {
                deleted_at: Some(now),
                updated_at: now,
                ..old.meta.clone()
            })?;
            Ok(())
        })?;
        history.index.remove_game(old.meta.uuid)?;
        history.refresh_game(new_uuid)?;
        let id = history
            .index
            .game_id(new_uuid)?
            .context("linked game is not indexed")?;
        for pointer in pointers {
            let Some(slot_id) = history.index.slot_id(pointer.slot)? else {
                continue;
            };
            let version_id = pointer
                .version
                .map(|v| history.index.version_id(v))
                .transpose()?
                .flatten();
            history.index.set_active_version(slot_id, version_id)?;
            if pointer.active {
                history.index.set_active_slot(id, slot_id)?;
            }
        }
        Ok(())
    }

    /// Starts tracking a remote game here. Existing local saves are kept as
    /// a new version; otherwise the newest device's progress is restored.
    /// Returns whether progress was applied.
    fn adopt(&self, remote: &Remote, remote_game: &GameRecord) -> Result<bool> {
        self.pull_game(remote, remote_game)?;
        let history = self.history();
        let game_id = history
            .index
            .game_id(remote_game.meta.uuid)?
            .context("Adopted game is not indexed")?;
        let me = self.device_metadata().device_uuid;
        if let Some(state) = remote
            .other_states(me)
            .into_iter()
            .find_map(|state| state.games.get(&remote_game.meta.uuid))
        {
            for (slot_uuid, slot_state) in &state.slots {
                let Some(slot_id) = history.index.slot_id(*slot_uuid)? else {
                    continue;
                };
                let version_id = match slot_state.active_version_uuid {
                    Some(uuid) => history.index.version_id(uuid)?,
                    None => None,
                };
                let usable = match version_id {
                    Some(id) => !history.index.get_version(id)?.version.retired,
                    None => false,
                };
                if usable {
                    history.index.set_active_version(slot_id, version_id)?;
                }
                if state.active_slot_uuid == Some(*slot_uuid) {
                    history.index.set_active_slot(game_id, slot_id)?;
                }
            }
        } else if let Some(slot) = self.get_game(game_id)?.active_slot {
            // No device points into it (the last one untracked it): resume
            // from the newest version rather than start a second root.
            let newest = self
                .get_slot(slot.id)?
                .versions
                .into_iter()
                .find(|version| !version.retired);
            history
                .index
                .set_active_version(slot.id, newest.map(|version| version.id))?;
        }
        // Decide under suppression and the lock, so saves written while the
        // pull ran are seen and never overwritten.
        self.exclusive(|| {
            let game = self.get_game(game_id)?;
            let version = game
                .last_version
                .as_ref()
                .map(|active| self.get_version(active.id))
                .transpose()?;
            // Every folder the restore would write, retired ones included.
            let live = self.covering(game.clone(), version.as_ref())?;
            let saves_exist = live.tracked_dirs.iter().any(|dir| {
                let path = crate::path_placeholders::expand_path_placeholders(&dir.path);
                std::fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_some())
            });
            if saves_exist {
                // Keep what is on disk: identical saves just join the remote
                // history; different ones become a new version on top of it.
                self.inner.checkpoint(&live, VersionReason::Initial)?;
                return Ok(false);
            }
            let Some(version) = version else {
                return Ok(false);
            };
            // The folders it writes are empty, so there is nothing to checkpoint first.
            self.restore_locked(&game, &version)?;
            Ok(true)
        })
    }

    /// Moves slots other devices advanced: inactive slots by pointer only, the
    /// active slot by a guarded restore when the game is not running.
    fn fast_forward(
        &self,
        remote: &Remote,
        remote_game: &GameRecord,
        game: &FullGame,
        install: impl Fn() -> Option<String>,
    ) -> Result<bool> {
        let me = self.device_metadata().device_uuid;
        let parents = sync::parents(&self.history().record(&game.game)?);
        let pointers = self.history().index.pointers()?;
        let mut applied = false;
        for pointer in pointers.iter().filter(|p| p.game == remote_game.meta.uuid) {
            let tips: Vec<Uuid> = remote
                .other_states(me)
                .iter()
                .filter_map(|state| state.slot_tip(pointer.game, pointer.slot))
                .collect();
            let (relation, target) = sync::classify(pointer.version, &tips, &parents);
            if relation != Divergence::FastForward {
                continue;
            }
            let Some(target_id) = target
                .map(|t| self.history().index.version_id(t))
                .transpose()?
                .flatten()
            else {
                continue;
            };
            let target = self.get_version(target_id)?;
            if target.version.retired {
                continue;
            }
            // An empty tip over saves here is more likely a wipe elsewhere
            // than progress: it stays a divergence for the user to choose.
            let current = pointer
                .version
                .map(|v| self.history().index.version_id(v))
                .transpose()?
                .flatten();
            if let Some(current) = current
                && !self
                    .history()
                    .version_meta(&self.get_version(current)?)?
                    .files
                    .is_empty()
                && self.history().version_meta(&target)?.files.is_empty()
            {
                continue;
            }
            if !pointer.active {
                self.history()
                    .index
                    .set_active_version(target.slot.id, Some(target_id))?;
                continue;
            }
            // Unknown process state counts as running: fail closed.
            if sync::game_is_running(install().as_deref()) != Some(false) {
                continue;
            }
            let expected_tip = pointer.version.map(|v| v.to_string());
            applied |= self.guarded(
                game.game.id,
                VersionReason::PreRestore,
                Some(&target),
                |game| {
                    // A checkpoint that just captured new local progress turns this into a divergence.
                    if game.last_version.as_ref().map(|v| v.uuid.clone()) != expected_tip {
                        return Ok(false);
                    }
                    self.restore_locked(game, &target)?;
                    Ok(true)
                },
            )?;
        }
        Ok(applied)
    }

    fn sync_report(
        &self,
        remote: &Remote,
        auto_applied: &HashSet<Uuid>,
        installed_titles: &HashMap<String, Option<String>>,
        problems: Vec<String>,
    ) -> Result<SyncReport> {
        let me = self.device_metadata().device_uuid;
        let catalog = remote.store.catalog()?;
        let pointers = self.history().index.pointers()?;
        let games = self.get_games()?;
        let mut statuses = Vec::new();
        for game in &games {
            let uuid = game_uuid(&game.game)?;
            let Some(remote_game) = catalog.iter().find(|g| g.meta.uuid == uuid) else {
                continue;
            };
            let Some(pointer) = pointers.iter().find(|p| p.game == uuid && p.active) else {
                continue;
            };
            let parents = sync::parents(remote_game);
            let others = remote.other_states(me);
            let tips: Vec<(Uuid, Uuid)> = others
                .iter()
                .filter_map(|state| {
                    state
                        .slot_tip(uuid, pointer.slot)
                        .map(|tip| (state.device_uuid, tip))
                })
                .collect();
            let (relation, target) = sync::classify(
                pointer.version,
                &tips.iter().map(|(_, tip)| *tip).collect::<Vec<_>>(),
                &parents,
            );
            // Nothing newer elsewhere, and this device published where it is: the cloud
            // has it, even with no other device to compare against.
            let published = remote
                .states
                .iter()
                .find(|state| state.device_uuid == me)
                .and_then(|state| state.slot_tip(uuid, pointer.slot));
            let relation = match relation {
                sync::Divergence::LocalOnly | sync::Divergence::LocalAhead
                    if published.is_some() && published == pointer.version =>
                {
                    sync::Divergence::InSync
                }
                relation => relation,
            };
            let device = target
                .and_then(|t| tips.iter().find(|(_, tip)| *tip == t))
                .map(|(device, _)| remote.device_name(*device));
            statuses.push(sync::GameSyncStatus {
                game_uuid: uuid,
                relation,
                remote_device: device,
                remote_version_uuid: target,
                remote_version_id: target
                    .map(|t| self.history().index.version_id(t))
                    .transpose()?
                    .flatten(),
                auto_applied: auto_applied.contains(&uuid),
            });
        }
        let local: HashSet<String> = games.iter().map(|g| g.game.uuid.clone()).collect();
        Ok(SyncReport {
            synced_at: Utc::now(),
            games: statuses,
            remote_games: catalog
                .iter()
                .filter(|g| g.meta.deleted_at.is_none())
                .map(|g| sync::RemoteGameSummary {
                    uuid: g.meta.uuid,
                    title: g.meta.title.clone(),
                    installed: installed_titles
                        .contains_key(&store::normalize_display_name(&g.meta.title)),
                    adopted: local.contains(&g.meta.uuid.to_string()),
                    source_device: g
                        .versions()
                        .max_by_key(|v| v.created_at)
                        .and_then(|v| v.origin_device)
                        .and_then(|origin| {
                            remote
                                .devices
                                .iter()
                                .find(|d| d.device_uuid == origin)
                                .map(|d| d.name.clone())
                        }),
                    version_count: g.versions().count(),
                })
                .collect(),
            devices: remote.devices.clone(),
            remote_versions: catalog
                .iter()
                .flat_map(|g| g.versions())
                .filter_map(|v| {
                    let origin = v.origin_device.filter(|d| *d != me)?;
                    Some((v.uuid, remote.device_name(origin)))
                })
                .collect(),
            problems,
        })
    }

    // ---- retention -----------------------------------------------------------------

    fn retention_path(&self) -> PathBuf {
        self.data_dir.join("retention_settings.json")
    }

    pub fn get_pruning_settings(&self) -> Result<RetentionSettings> {
        maintenance::sync_settings(&self.retention_path(), None)
    }

    pub fn update_pruning_settings(
        &self,
        settings: RetentionSettings,
    ) -> Result<RetentionSettings> {
        validate_retention(&settings)?;
        let settings = RetentionSettings {
            updated_at: Utc::now(),
            ..settings
        };
        write_json(&self.retention_path(), &settings)?;
        if let Ok(Some(remote)) = self.remote() {
            maintenance::sync_settings(&self.retention_path(), Some(&remote.store))?;
        }
        Ok(settings)
    }

    pub fn retired_versions(&self) -> Result<Vec<String>> {
        self.history().index.retired_version_uuids()
    }

    /// The union catalog, retired set and protected pointers used for planning.
    /// Sizes for budget planning: local chunks overlaid with the remote's, so
    /// remote-only chunks count and every device plans with the same numbers.
    fn planning_chunk_sizes(&self, remote: Option<&Remote>) -> Result<HashMap<String, u64>> {
        let mut sizes = self.history().store.chunk_sizes()?;
        if let Some(remote) = remote {
            sizes.extend(remote.store.chunk_sizes()?);
        }
        Ok(sizes)
    }

    fn retention_inputs(&self, remote: Option<&Remote>) -> Result<RetentionInputs> {
        let history = self.history();
        let mut games = remote
            .map(|r| r.store.catalog())
            .transpose()?
            .unwrap_or_default();
        for local in history.store.catalog()? {
            match games.iter_mut().find(|g| g.meta.uuid == local.meta.uuid) {
                None => games.push(local),
                Some(merged) => {
                    for slot in local.slots {
                        match merged
                            .slots
                            .iter_mut()
                            .find(|s| s.meta.uuid == slot.meta.uuid)
                        {
                            None => merged.slots.push(slot),
                            Some(target) => {
                                for version in slot.versions {
                                    if !target.versions.iter().any(|v| v.uuid == version.uuid) {
                                        target.versions.push(version);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        let mut retired: HashSet<Uuid> = history.store.markers()?.into_keys().collect();
        let local_pointed: HashSet<Uuid> = history
            .index
            .pointers()?
            .iter()
            .filter_map(|p| p.version)
            .collect();
        let mut pointed = local_pointed.clone();
        if let Some(remote) = remote {
            retired.extend(remote.store.markers()?.into_keys());
            pointed.extend(remote.states.iter().flat_map(|s| s.pointed_versions()));
        }
        Ok(RetentionInputs {
            games,
            retired,
            pointed,
            local_pointed,
        })
    }

    /// What pruning with `settings` (saved or not) would do. Writes nothing.
    pub fn preview_pruning(&self, settings: &RetentionSettings) -> Result<PruningPreview> {
        validate_retention(settings)?;
        let remote = self.remote()?;
        let inputs = self.retention_inputs(remote.as_ref())?;
        let plan = maintenance::plan(&maintenance::PlanInput {
            games: &inputs.games,
            retired: &inputs.retired,
            pointed: &inputs.pointed,
            chunk_sizes: &self.planning_chunk_sizes(remote.as_ref())?,
            settings,
            now: planning_now(),
        });
        Ok(PruningPreview {
            retire_versions: plan.retire.iter().map(|(_, _, v)| *v).collect(),
            live_bytes_before: plan.live_bytes_before,
            live_bytes_after: plan.live_bytes_after,
            budget_unreachable: plan.budget_unreachable,
            local_gc: maintenance::gc_preview(&self.history().store, &inputs.local_pointed)?,
        })
    }

    /// Retires versions per the retention settings, then garbage-collects the
    /// local store and (condemn → sweep) the remote.
    pub fn run_pruning(&self) -> Result<PruningRunReport> {
        self.run_pruning_at(planning_now())
    }

    /// `run_pruning` with an explicit clock, for tests of time-based retention.
    #[doc(hidden)]
    pub fn run_pruning_at(&self, now: DateTime<Utc>) -> Result<PruningRunReport> {
        let remote = self.remote()?;
        let _guard = self.sync_lock.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(remote) = &remote {
            if let Some(path) = &remote.unreadable_state {
                bail!(
                    "{} is missing or unreadable; cleanup is skipped until it is fixed",
                    path.display()
                );
            }
            let problems = self.push_locked(remote)?;
            if !problems.is_empty() {
                bail!(
                    "Cleanup needs a complete push first: {}",
                    problems.join("; ")
                );
            }
        }
        let history = self.history();
        let settings =
            maintenance::sync_settings(&self.retention_path(), remote.as_ref().map(|r| &r.store))?;
        let (retired, budget_unreachable, local_gc) = store::with_lock(history.root(), || {
            let inputs = self.retention_inputs(remote.as_ref())?;
            let plan = maintenance::plan(&maintenance::PlanInput {
                games: &inputs.games,
                retired: &inputs.retired,
                pointed: &inputs.pointed,
                chunk_sizes: &self.planning_chunk_sizes(remote.as_ref())?,
                settings: &settings,
                now,
            });
            maintenance::apply(&history.store, &plan, now)?;
            if let Some(remote) = &remote {
                maintenance::apply(&remote.store, &plan, now)?;
            }
            let local_gc = maintenance::gc_local(&history.store, &inputs.local_pointed)?;
            Ok((
                plan.retire.iter().map(|(_, _, v)| *v).collect::<Vec<_>>(),
                plan.budget_unreachable,
                local_gc,
            ))
        })?;
        let remote_gc = match &remote {
            Some(remote) => {
                let mut pointed: HashSet<Uuid> = history
                    .index
                    .pointers()?
                    .iter()
                    .filter_map(|p| p.version)
                    .collect();
                pointed.extend(remote.states.iter().flat_map(|s| s.pointed_versions()));
                Some(maintenance::gc_remote(
                    &remote.store,
                    &pointed,
                    self.device_metadata().device_uuid,
                    now,
                )?)
            }
            None => None,
        };
        history.reindex()?;
        *self.last_prune.lock().unwrap_or_else(|e| e.into_inner()) = Some(now);
        Ok(PruningRunReport {
            retired_versions: retired,
            budget_unreachable,
            local_gc,
            remote_gc,
        })
    }

    /// Runs pruning when automatic retention is on and the last run is older
    /// than the interval. Returns the report when it ran.
    pub fn prune_if_due(&self) -> Result<Option<PruningRunReport>> {
        if !self.get_pruning_settings()?.automatic {
            return Ok(None);
        }
        let last = *self.last_prune.lock().unwrap_or_else(|e| e.into_inner());
        if last.is_some_and(|at| Utc::now() - at < PRUNE_INTERVAL) {
            return Ok(None);
        }
        self.run_pruning().map(Some)
    }
}

impl Drop for WorldlineCore {
    fn drop(&mut self) {
        self.shutdown();
    }
}

enum LocalMatch {
    None,
    Same(Box<FullGame>),
    Collision(String),
}

/// What retention planning reads: the union catalog of local and remote,
/// every retired version, and the pointers of all devices and of this one.
struct RetentionInputs {
    games: Vec<GameRecord>,
    retired: HashSet<Uuid>,
    pointed: HashSet<Uuid>,
    local_pointed: HashSet<Uuid>,
}

fn validate_retention(settings: &RetentionSettings) -> Result<()> {
    if settings.keep_all_hours > settings.hourly_until_hours
        || settings.hourly_until_hours > settings.daily_until_days * 24
        || settings.daily_until_days > settings.weekly_until_days
    {
        bail!("Retention tiers must not overlap: keep-all ≤ hourly ≤ daily ≤ weekly");
    }
    Ok(())
}

/// Retention decisions depend on `now` truncated to the hour, so devices
/// planning within the same hour agree.
fn planning_now() -> DateTime<Utc> {
    use chrono::DurationRound;
    let now = Utc::now();
    now.duration_trunc(chrono::Duration::hours(1))
        .unwrap_or(now)
}

fn open_database(path: &Path) -> Result<DbPool> {
    let url = format!("file:{}?foreign_keys=on", path.display());
    let pool = Pool::builder()
        .max_size(4)
        .connection_customizer(Box::new(SqlitePragmas))
        .build(ConnectionManager::<SqliteConnection>::new(url))
        .context("Failed to open the database")?;
    pool.get()?
        .run_pending_migrations(crate::MIGRATIONS)
        .map_err(|error| anyhow!("Failed to migrate the database: {error}"))?;
    Ok(pool)
}

#[derive(Debug)]
struct SqlitePragmas;

impl diesel::r2d2::CustomizeConnection<SqliteConnection, diesel::r2d2::Error> for SqlitePragmas {
    fn on_acquire(
        &self,
        conn: &mut SqliteConnection,
    ) -> std::result::Result<(), diesel::r2d2::Error> {
        use diesel::connection::SimpleConnection;
        conn.batch_execute("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;")
            .map_err(diesel::r2d2::Error::QueryError)
    }
}

/// Pre-release data (an older schema or store format) is moved into
/// `backup-pre-fase3-<timestamp>/`, never deleted, and the app starts fresh.
fn move_aside_old_data(data_dir: &Path) -> Result<Option<PathBuf>> {
    let db = data_dir.join("database.db");
    let repository = data_dir.join("repository");
    let old_db = db.exists() && {
        use diesel::Connection;
        use diesel::prelude::*;
        let mut conn = SqliteConnection::establish(&db.to_string_lossy())?;
        #[derive(QueryableByName)]
        struct Row {
            #[diesel(sql_type = diesel::sql_types::Text)]
            version: String,
        }
        use diesel::migration::MigrationSource;
        let known: Vec<String> =
            MigrationSource::<diesel::sqlite::Sqlite>::migrations(&crate::MIGRATIONS)
                .map_err(|error| anyhow!("{error}"))?
                .iter()
                .map(|migration| migration.name().version().to_string())
                .collect();
        let applied: Vec<Row> = diesel::sql_query("SELECT version FROM __diesel_schema_migrations")
            .load(&mut conn)
            .unwrap_or_default();
        applied.iter().any(|row| !known.contains(&row.version))
    };
    let old_repository =
        repository.join("metadata.json").exists() && Store::open_existing(&repository).is_err();
    if !old_db && !old_repository {
        return Ok(None);
    }
    let backup = data_dir.join(format!(
        "backup-pre-fase3-{}-{}",
        Utc::now().format("%Y%m%d-%H%M%S"),
        &store::short_random()[..4]
    ));
    std::fs::create_dir_all(&backup)
        .with_context(|| format!("Failed to create {}", backup.display()))?;
    for name in [
        "database.db",
        "database.db-wal",
        "database.db-shm",
        "repository",
    ] {
        let source = data_dir.join(name);
        if source.exists() {
            std::fs::rename(&source, backup.join(name))
                .with_context(|| format!("Failed to move {} aside", source.display()))?;
        }
    }
    Ok(Some(backup))
}

#[cfg(target_os = "windows")]
fn extract_exe_icon(exe_path: &Path, output_path: &Path) -> Result<()> {
    use std::os::windows::process::CommandExt;
    let script = "Add-Type -AssemblyName System.Drawing; \
                  $icon = [System.Drawing.Icon]::ExtractAssociatedIcon($env:EXE_PATH); \
                  $icon.ToBitmap().Save($env:OUT_PATH)";
    // CREATE_NO_WINDOW: the GUI is a windowed app, so a console would flash per icon.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let status = std::process::Command::new("powershell")
        .creation_flags(CREATE_NO_WINDOW)
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("EXE_PATH", exe_path)
        .env("OUT_PATH", output_path)
        .status()
        .context("Failed to run PowerShell")?;
    anyhow::ensure!(status.success(), "PowerShell icon extraction failed");
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn extract_exe_icon(_: &Path, _: &Path) -> Result<()> {
    bail!("Executable icon extraction is only supported on Windows")
}
