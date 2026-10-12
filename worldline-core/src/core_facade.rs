use crate::bundle::{HistoryBundleSummary, HistoryTransferReport};
use crate::detection::running::RunningMonitor;
use crate::detection::{
    BranchFrom, CaptureTarget, Checkpoints, Notification, NotificationReceiver, RunningGames,
    Suppression, Watcher,
};
use crate::error::ErrorKind;
use crate::history::{CheckpointOutcome, History, Kept, game_uuid};
use crate::index::{DbPool, Index};
use crate::install_scanner::InstallScannerItem;
use crate::maintenance::{self, GcReport, RetentionSettings};
use crate::models::*;
use crate::restore;
use crate::store::{self, GameRecord, VersionReason, read_json, write_json};
use crate::sync::{
    self, DeviceKind, DeviceMetadata, DeviceState, Divergence, Remote, SyncReport, SyncSettings,
};
use crate::sync_loop::{SyncLoop, Trigger};
use crate::transfer::{Completeness, Heal, Transfer};
use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, NaiveDateTime, Utc};
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

/// One install a scan found: where it is, and where its icon comes from.
#[derive(Debug, Clone, Default)]
struct ScannedInstall {
    /// The title the scan found it by, naming its preview in the icon cache.
    title: String,
    install_path: Option<String>,
    icon_path: Option<String>,
    steam_appid: Option<u64>,
}

impl ScannedInstall {
    /// The scan's result keyed by normalized title, as installs are matched to games.
    fn by_title(items: &[InstallScannerItem]) -> HashMap<String, Self> {
        items
            .iter()
            .map(|item| {
                (
                    store::normalize_display_name(&item.title),
                    Self {
                        title: item.title.clone(),
                        install_path: item.install_path.clone(),
                        icon_path: item.icon_path.clone(),
                        steam_appid: item.steam_appid,
                    },
                )
            })
            .collect()
    }

    fn icon(&self) -> Option<crate::icon::Source<'_>> {
        let local = Path::new(self.icon_path.as_deref()?);
        Some(match self.steam_appid {
            Some(appid) => crate::icon::Source::Steam { appid, local },
            None => crate::icon::Source::File(local),
        })
    }
}

/// A game here that wants an icon (see `games_wanting_icons`), and the
/// install the scan found it by, if any.
struct WantedIcon {
    uuid: Uuid,
    title_normalized: String,
    install: ScannedInstall,
    /// The size of its automatic icon; `None` when it has none.
    size: Option<(u32, u32)>,
    /// Whether Steam may be asked about it now (`lookup_due`).
    due: bool,
}

impl WantedIcon {
    /// Whether it's worth an install scan or a sync of its own: it has no
    /// icon, or Steam may be asked about it. An automatic icon looked up
    /// lately waits for the next scan or sync that happens anyway.
    fn is_pressing(&self) -> bool {
        self.size.is_none() || self.due
    }
}

/// A game looked up on Steam is looked up again after this long: one Steam
/// had no icon for, or one whose icon is still smaller than `ICON_SIZE`.
const ICON_LOOKUP_RETRY: chrono::Duration = chrono::Duration::days(7);

/// When each game was last looked up on Steam, from `cache/icon_lookups.json`.
fn read_icon_lookups(path: &Path) -> Result<HashMap<Uuid, DateTime<Utc>>> {
    match read_json(path) {
        Ok(lookups) => Ok(lookups),
        Err(_) if !path.exists() => Ok(HashMap::new()),
        Err(error) => Err(error),
    }
}

/// Whether `uuid` may be looked up on Steam at `now`.
fn lookup_due(lookups: &HashMap<Uuid, DateTime<Utc>>, uuid: &Uuid, now: DateTime<Utc>) -> bool {
    lookups
        .get(uuid)
        .is_none_or(|at| now - *at >= ICON_LOOKUP_RETRY)
}

/// The Add Games preview cache, shared with the icon fills.
fn icon_cache_dir(cache_dir: &Path) -> PathBuf {
    let dir = cache_dir.join("icon_cache_v2");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// The name of `title`'s preview in the icon cache.
fn preview_name(title: &str) -> String {
    title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// How far back a push heals versions that were created or pulled.
fn heal_window() -> chrono::Duration {
    maintenance::GRACE * 2
}

/// What the last sync report read from a remote.
struct LastReport {
    root: PathBuf,
    catalog: Vec<GameRecord>,
    statuses: Vec<sync::GameSyncStatus>,
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
    /// Games whose live saves differ from their active version, with when the
    /// watcher captures them. Device-local.
    unrecorded: Mutex<HashMap<i32, NaiveDateTime>>,
    /// Where changes to `unrecorded` are notified, while the watcher runs;
    /// dropped on shutdown so the notifications end.
    notifications: Mutex<Option<mpsc::Sender<Notification>>>,
}

impl Inner {
    /// The one way to capture live saves: a created version queues a push,
    /// and the watcher hears of every outcome (for screenshots).
    /// First it finishes putting back saves a failed write left half
    /// written; while that can't finish, the game isn't captured, so the
    /// half-written folder is never recorded, and the watcher retries.
    /// `limited` refuses saves far larger than usual: for the captures nobody
    /// asked for, and a new game's first one until the user confirms its size.
    fn checkpoint(
        &self,
        game: &FullGame,
        reason: VersionReason,
        limited: bool,
    ) -> Result<CheckpointOutcome> {
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
        let outcome = self.history.checkpoint_locked(game, reason, limited)?;
        if matches!(outcome, CheckpointOutcome::Created(_))
            && let Ok(uuid) = game_uuid(&game.game)
        {
            let _ = self.sync_trigger.send(Trigger::Push(uuid));
        }
        if let Some(checkpoints) = self.checkpoints.get() {
            checkpoints.completed(game.game.id, started, &outcome);
        }
        // Whatever the outcome, the active version now holds the live saves.
        self.set_unrecorded(game.game.id, None);
        Ok(outcome)
    }

    /// Sets the game's unrecorded changes to `due_at`, or clears them with
    /// `None`, and notifies when that changes them.
    fn set_unrecorded(&self, game_id: i32, due_at: Option<NaiveDateTime>) {
        let mut unrecorded = self.unrecorded.lock().unwrap_or_else(|e| e.into_inner());
        let changed = match due_at {
            Some(due_at) => unrecorded.insert(game_id, due_at) != Some(due_at),
            None => unrecorded.remove(&game_id).is_some(),
        };
        // Still under the lock, so notifications arrive in the order of the changes.
        if changed
            && let Some(notifications) =
                &*self.notifications.lock().unwrap_or_else(|e| e.into_inner())
        {
            let _ = notifications.send(Notification::UnrecordedChanges { game_id, due_at });
        }
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

    fn capture(&self, game_id: i32) -> Result<(CheckpointOutcome, Option<BranchFrom>)> {
        store::with_lock(self.history.root(), || {
            let game = self.history.index.get_game(game_id)?;
            let outcome = self.checkpoint(&game, VersionReason::Watcher, true)?;
            // The version exists either way: a failed lookup only loses the hint.
            let branch_from = match &outcome {
                CheckpointOutcome::Created(version) => {
                    self.history.branch_from(version).ok().flatten()
                }
                _ => None,
            };
            Ok((outcome, branch_from))
        })
    }

    fn attach_screenshot(&self, version_id: i32, jpeg: &[u8]) -> Result<()> {
        store::with_lock(self.history.root(), || {
            self.history.write_screenshot(version_id, jpeg)
        })
    }

    /// Under the repository lock, so no checkpoint lands between the check
    /// and the update.
    fn check_unrecorded(&self, game_id: i32, due_at: NaiveDateTime) -> Result<()> {
        store::with_lock(self.history.root(), || {
            let game = self.history.index.get_game(game_id)?;
            let differs = self.history.live_differs(&game)?;
            self.set_unrecorded(game_id, differs.then_some(due_at));
            Ok(())
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
    last_installs: Mutex<HashMap<String, ScannedInstall>>,
    /// The last report's remote, so the report after a game's push re-reads
    /// only that game.
    last_report: Mutex<Option<LastReport>>,
    /// What the last install scan found, by normalized title; `None` until
    /// this process scans.
    scanned_installs: Mutex<Option<HashMap<String, ScannedInstall>>>,
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
        let (tx, rx) = mpsc::channel();
        let inner = Arc::new(Inner {
            history,
            suppression: Suppression::default(),
            txn_root: data_dir.join("txn"),
            sync_trigger,
            checkpoints: OnceLock::new(),
            unrecorded: Mutex::new(HashMap::new()),
            notifications: Mutex::new(None),
        });
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
            last_report: Mutex::new(None),
            scanned_installs: Mutex::new(None),
            last_prune: Mutex::new(None),
            moved_old_data,
            recovered_restores,
        };
        if options.watch {
            let mut settings = core.get_monitoring_settings()?;
            if let Some(seconds) = options.debounce_seconds {
                settings.snapshot_debounce_seconds = seconds;
            }
            *core
                .inner
                .notifications
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = Some(tx.clone());
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
        self.inner
            .notifications
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
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

    /// The games whose live saves differ from their active version, with when
    /// the watcher captures them; empty unless opened with `watch`. Changes
    /// are also notified; every checkpoint and restore clears its game's.
    pub fn unrecorded_changes(&self) -> Vec<(i32, NaiveDateTime)> {
        let unrecorded = self
            .inner
            .unrecorded
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut changes: Vec<_> = unrecorded.iter().map(|(&id, &at)| (id, at)).collect();
        changes.sort_unstable();
        changes
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
        let linked = self.linked_slots();
        self.history()
            .index
            .get_games()?
            .into_iter()
            .map(|game| with_linked(game, &linked))
            .collect()
    }

    pub fn get_game(&self, id: i32) -> Result<FullGame> {
        with_linked(self.history().index.get_game(id)?, &self.linked_slots())
    }

    fn linked_path(&self) -> PathBuf {
        self.data_dir.join("linked_slots.json")
    }

    /// Game UUID → slots another device's history brought in at linking.
    fn linked_slots(&self) -> HashMap<Uuid, Vec<Uuid>> {
        read_json(&self.linked_path()).unwrap_or_default()
    }

    fn set_linked_slots(&self, game: Uuid, slots: Vec<Uuid>) -> Result<()> {
        let mut linked = self.linked_slots();
        let changed = if slots.is_empty() {
            linked.remove(&game).is_some()
        } else {
            linked.insert(game, slots);
            true
        };
        if changed {
            write_json(&self.linked_path(), &linked)?;
        }
        Ok(())
    }

    /// Forgets the playthroughs another device brought in, so they are no
    /// longer pointed out.
    pub fn dismiss_linked_slots(&self, game_id: i32) -> Result<FullGame> {
        let game = self.get_game(game_id)?;
        self.set_linked_slots(game_uuid(&game.game)?, Vec::new())?;
        self.get_game(game_id)
    }

    pub fn get_slot(&self, id: i32) -> Result<FullSlot> {
        self.history().index.get_slot(id)
    }

    pub fn get_version(&self, id: i32) -> Result<FullVersion> {
        self.history().index.get_version(id)
    }

    /// Creates a game (capturing its current saves as the first version) or
    /// edits one. `dirs`, when given, becomes the set of tracked directories.
    /// Under the repository lock, so two adds can't both pass the name and
    /// folder checks before either is written. Saves far larger than a game's
    /// usually are refuse the add with `ErrorKind::TooLarge`; `save_large_game`
    /// takes them anyway.
    pub fn save_game(&self, form: GameForm, dirs: Option<Vec<String>>) -> Result<Game> {
        self.save_game_limited(form, dirs, true)
    }

    /// `save_game` for saves the user confirmed are this large.
    pub fn save_large_game(&self, form: GameForm, dirs: Option<Vec<String>>) -> Result<Game> {
        self.save_game_limited(form, dirs, false)
    }

    fn save_game_limited(
        &self,
        form: GameForm,
        dirs: Option<Vec<String>>,
        limited: bool,
    ) -> Result<Game> {
        let game = store::with_lock(self.repository_root(), || {
            self.write_game(form, dirs, limited)
        })?;
        self.reload_watcher();
        // A game added from the scan, or renamed to an installed title, gets its install dir.
        self.refresh_install_dirs()?;
        Ok(self.get_game(game.game.id)?.game)
    }

    /// `save_game`'s writes; the caller holds the repository lock.
    fn write_game(
        &self,
        form: GameForm,
        dirs: Option<Vec<String>>,
        limited: bool,
    ) -> Result<FullGame> {
        Ok(match form.id {
            None => {
                let game = self
                    .history()
                    .create_game(&form.title, dirs.as_deref().unwrap_or_default())?;
                if !game.tracked_dirs.is_empty() {
                    let first = self
                        .inner
                        .checkpoint(&game, VersionReason::Initial, limited);
                    if let Err(error) = first {
                        // Not added after all, so trying again isn't blocked by its name or folders.
                        let history = self.history();
                        if let Err(undo) = history
                            .delete_game(game.game.id)
                            .and_then(|()| history.store.drop_game_history(&game.game.folder_slug))
                        {
                            eprintln!("Couldn't take back '{}': {undo:#}", game.game.title);
                        }
                        return Err(error);
                    }
                }
                game
            }
            Some(id) => self
                .history()
                .update_game(id, &form.title, dirs.as_deref())?,
        })
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
                    .checkpoint(&self.get_game(id)?, VersionReason::Watcher, false)
                    .context("Failed to checkpoint the current saves first")?;
            }
            self.history().set_untracked(uuid, true)?;
            self.history().refresh_game(uuid)
        })?;
        // Publishes the captured progress, and pointers without the game.
        for problem in self.push_locked(&remote, None)? {
            eprintln!("Sync: {problem}");
        }
        Ok(())
    }

    /// Under the repository lock, like `keep`, so a keep can't overwrite the name.
    pub fn tag_version(&self, id: i32, tag: String) -> Result<Version> {
        store::with_lock(self.repository_root(), || {
            self.history().tag_version(id, Some(tag))
        })
    }

    /// Checkpoints the live saves now (a no-op when unchanged).
    pub fn checkpoint(&self, game_id: i32) -> Result<CheckpointOutcome> {
        store::with_lock(self.repository_root(), || {
            let game = self.get_game(game_id)?;
            self.inner.checkpoint(&game, VersionReason::Manual, false)
        })
    }

    /// Checkpoints the live saves now, like `checkpoint`, and tags the new
    /// version with `tag`, which keeps it forever; when the saves haven't
    /// changed, the active version. A version that already
    /// has a tag keeps it. One repository lock covers both, and every tag
    /// write takes it, so nothing lands, prunes or renames in between. Only a
    /// failed capture is an error: a created version is reported even when
    /// tagging it then fails. `still` is asked first, under the lock: when it
    /// says the keep no longer stands, nothing is touched and it's `None`.
    pub fn keep(
        &self,
        game_id: i32,
        tag: &str,
        still: impl FnOnce() -> bool,
    ) -> Result<Option<Kept>> {
        self.keep_with(game_id, tag, still, |history, id| {
            history.tag_if_untagged(id, tag)
        })
    }

    /// `keep`, with the tag step given, so tests can make it fail.
    fn keep_with(
        &self,
        game_id: i32,
        tag: &str,
        still: impl FnOnce() -> bool,
        tag_step: impl FnOnce(&History, i32) -> Result<Version>,
    ) -> Result<Option<Kept>> {
        // A blank tag would clear one instead of keeping the version.
        if tag.trim().is_empty() {
            bail!("A kept checkpoint needs a name");
        }
        store::with_lock(self.repository_root(), || {
            if !still() {
                return Ok(None);
            }
            let game = self.get_game(game_id)?;
            let checkpoint = self.inner.checkpoint(&game, VersionReason::Manual, false)?;
            let branch_from = match &checkpoint {
                // The version exists either way: a failed lookup only loses the hint.
                CheckpointOutcome::Created(version) => {
                    self.history().branch_from(version).ok().flatten()
                }
                _ => None,
            };
            // Unchanged keeps the active version, the one the saves match. After a restore the
            // slot's newest is on the branch the player left.
            let target = match &checkpoint {
                CheckpointOutcome::Created(version) | CheckpointOutcome::Unchanged(version) => {
                    Some(version.id)
                }
                CheckpointOutcome::Empty => None,
            };
            let tagged = target.map(|id| {
                tag_step(self.history(), id).inspect_err(|_| {
                    // The tag may be written but not indexed: show what the files say.
                    if let Ok(uuid) = game_uuid(&game.game) {
                        self.history().refresh_game(uuid).ok();
                    }
                })
            });
            Ok(Some(Kept {
                checkpoint,
                branch_from,
                tagged,
            }))
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
                    .checkpoint(&game, reason, false)
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
                self.inner
                    .checkpoint(&game, VersionReason::Initial, false)?;
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
            self.activate_slot(game, slot_id)
        })?;
        // Found the other device's playthrough: stop pointing it out.
        if game.linked_slot_ids.contains(&slot_id) {
            self.set_linked_slots(game_uuid(&game.game)?, Vec::new())?;
        }
        self.get_game(game_id)
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

    /// Stores an image (or an executable's icon) the user picked as the
    /// game's shared icon and returns its local path.
    pub fn set_game_icon(&self, game_id: i32, source_path: &str) -> Result<String> {
        self.set_game_icon_as(game_id, source_path, store::IconSource::User)
    }

    /// `set_game_icon`, chosen by `source`. An automatic icon (Add Games'
    /// preview) only takes the place of none or of a smaller automatic one
    /// (`History::takes_auto_icon`); otherwise the game keeps its icon and
    /// its path is returned.
    pub fn set_game_icon_as(
        &self,
        game_id: i32,
        source_path: &str,
        source: store::IconSource,
    ) -> Result<String> {
        let image = crate::icon::resolve(crate::icon::Source::File(Path::new(source_path)))?;
        let png = crate::icon::normalize(&image)?;
        let size = crate::icon::dimensions(&png)?;
        let game = store::with_lock(self.repository_root(), || {
            let history = self.history();
            if source == store::IconSource::Auto
                && !history.takes_auto_icon(&history.game_meta(game_id)?, size)
            {
                return history.index.get_game(game_id);
            }
            history.set_icon(game_id, &png, source)
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
    /// of the games already tracked here, and fills their missing icons
    /// (see `fill_missing_icons`).
    pub fn scan_installed(&self) -> Result<Vec<InstallScannerItem>> {
        let cache_dir = self.data_dir.join("cache");
        let candidates =
            crate::install_scanner::scan_installed_candidates(&cache_dir, &self.get_scan_roots()?)?;
        self.remember_installs(ScannedInstall::by_title(&candidates))?;
        if let Err(error) = self.fill_missing_icons() {
            eprintln!("Icons: {error:#}");
        }
        let mut items: Vec<InstallScannerItem> = candidates
            .into_iter()
            .filter(|item| !item.dirs.is_empty())
            .collect();
        // v1 held 32 px exe icons and 32 px Steam ones.
        let _ = std::fs::remove_dir_all(cache_dir.join("icon_cache"));
        let icon_cache_dir = icon_cache_dir(&cache_dir);
        // After one failed download, the rest of this scan uses local icons
        // instead of waiting out a timeout per game.
        let mut offline = false;
        for item in &mut items {
            let Some(source_path) = &item.icon_path else {
                continue;
            };
            let safe_title = preview_name(&item.title);
            let local = Path::new(source_path);
            let source = match item.steam_appid {
                Some(appid) => crate::icon::Source::Steam { appid, local },
                None => crate::icon::Source::File(local),
            };
            item.icon_path = cached_preview(
                &icon_cache_dir,
                &safe_title,
                source,
                None,
                &mut offline,
                crate::icon::download,
            )
            .map(|path| path.to_string_lossy().into_owned());
        }
        Ok(items)
    }

    /// Keeps `installs` as the last scan's result and refreshes install dirs from it.
    fn remember_installs(&self, installs: HashMap<String, ScannedInstall>) -> Result<()> {
        *self
            .scanned_installs
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(installs);
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
            .clone()
            .unwrap_or_default();
        let index = &self.history().index;
        for (id, title, stored) in index.install_dirs()? {
            let wanted = match scanned.get(&title).and_then(|s| s.install_path.clone()) {
                Some(dir) => Some(dir),
                None => stored.clone().filter(|dir| Path::new(dir).is_dir()),
            };
            if wanted != stored {
                index.set_install_dir(id, wanted.as_deref())?;
            }
        }
        Ok(())
    }

    /// Gives the games that want an icon (`games_wanting_icons`) the icon of
    /// their install, as the last install scan found it (ADR 0005). Without
    /// a remote that happens now. With one, the sync does it after pulling,
    /// so the write can't win over a rename another device made; a sync is
    /// asked for when there is something to fill. Games still wanting one
    /// then get their Steam icon (`fill_steam_icons`). Returns how many games
    /// got an icon.
    fn fill_missing_icons(&self) -> Result<usize> {
        let installs = self
            .scanned_installs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .unwrap_or_default();
        let now = Utc::now();
        if self.get_sync_settings()?.remote_path.is_some() {
            let wanted = self.wanted_install_icons(&installs, now)?;
            if wanted.iter().any(WantedIcon::is_pressing) {
                let _ = self.inner.sync_trigger.send(Trigger::Sync);
            }
            return Ok(0);
        }
        Ok(self.fill_icons(&installs, crate::icon::download, now)?
            + self.fill_steam_icons(crate::icon::download, now)?)
    }

    /// The games here that want an icon: those whose stored metadata names
    /// none, and those whose automatic icon is smaller than `ICON_SIZE`
    /// (ADR 0005). A user's icon, or one that hasn't arrived, wants nothing.
    /// Untracked and deleted games aren't indexed.
    fn games_wanting_icons(&self, now: DateTime<Utc>) -> Result<Vec<WantedIcon>> {
        let lookups_path = self.data_dir.join("cache").join("icon_lookups.json");
        let lookups = read_icon_lookups(&lookups_path).unwrap_or_default();
        let history = self.history();
        let mut wanted = Vec::new();
        for game in history.index.get_games()? {
            let Some(meta) = history.store.game_meta(&game.game.folder_slug)? else {
                continue;
            };
            let size = match meta.icon_sha256 {
                None => None,
                Some(_) if meta.icon_source == store::IconSource::Auto => {
                    match history.icon_size(&meta) {
                        Some((w, h)) if w.max(h) < crate::icon::ICON_SIZE => Some((w, h)),
                        _ => continue,
                    }
                }
                Some(_) => continue,
            };
            wanted.push(WantedIcon {
                uuid: meta.uuid,
                title_normalized: game.game.title_normalized,
                install: ScannedInstall::default(),
                size,
                due: lookup_due(&lookups, &meta.uuid, now),
            });
        }
        Ok(wanted)
    }

    /// The games here that want an icon whose install is in `installs` with one.
    fn wanted_install_icons(
        &self,
        installs: &HashMap<String, ScannedInstall>,
        now: DateTime<Utc>,
    ) -> Result<Vec<WantedIcon>> {
        let mut wanted = self.games_wanting_icons(now)?;
        wanted.retain_mut(|game| {
            match installs
                .get(&game.title_normalized)
                .filter(|install| install.icon_path.is_some())
            {
                Some(install) => {
                    game.install = install.clone();
                    true
                }
                None => false,
            }
        });
        Ok(wanted)
    }

    /// What the last install scan in this process found, for filling icons.
    /// A process that hasn't scanned yet (a CLI sync, or the app when no
    /// game wanted an icon at startup) scans now through `scan` when some
    /// game presses for one (`WantedIcon::is_pressing`), so its install's
    /// icon comes before Steam's.
    fn installs_for_icons(
        &self,
        scan: impl FnOnce() -> HashMap<String, ScannedInstall>,
        now: DateTime<Utc>,
    ) -> Result<HashMap<String, ScannedInstall>> {
        let scanned = self
            .scanned_installs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        Ok(match scanned {
            Some(scanned) => scanned,
            None if self
                .games_wanting_icons(now)?
                .iter()
                .any(WantedIcon::is_pressing) =>
            {
                scan()
            }
            None => HashMap::new(),
        })
    }

    /// Fills the icons of games just pulled or adopted that want one, before
    /// the push publishes them: after the pull, so the metadata written is
    /// current. An install's icon first, then Steam's. Never fails the sync.
    fn fill_icons_before_push(
        &self,
        installs: &HashMap<String, ScannedInstall>,
        fetch: impl Fn(&str) -> Result<Vec<u8>>,
        now: DateTime<Utc>,
    ) {
        if let Err(error) = self.fill_icons(installs, &fetch, now) {
            eprintln!("Icons: {error:#}");
        }
        if let Err(error) = self.fill_steam_icons(&fetch, now) {
            eprintln!("Steam icons: {error:#}");
        }
    }

    /// Gives the games still wanting an icon their Steam icon (ADR 0010):
    /// the manifest names each title's appid, one batched `GetItems` call
    /// (chunked past `STEAM_ITEMS_PER_CALL`) gives their `community_icon`
    /// hashes, and each icon is downloaded outside the repository lock and
    /// stored under it like `fill_icons` does. Runs after the install icons,
    /// so an install always wins.
    ///
    /// `cache/icon_lookups.json` keeps when each game was last looked up
    /// with an answer: no appid, no `community_icon`, neither icon size on
    /// the CDN, or an icon (sharper or not than the game's). Those aren't
    /// looked up again for `ICON_LOOKUP_RETRY`, so a game whose Steam icon is
    /// smaller than `ICON_SIZE` is asked about weekly, not every sync. A
    /// lookup that failed for now (no manifest yet, offline, throttled, a
    /// server error, an answer that isn't Steam's) is not kept, so the next
    /// sync tries again rather than a short outage hiding icons for a week;
    /// so is an icon download that failed any way but Steam not having it
    /// (a body that isn't an image included, as for the Add Games preview).
    /// Returns how many games got an icon.
    fn fill_steam_icons(
        &self,
        fetch: impl Fn(&str) -> Result<Vec<u8>>,
        now: DateTime<Utc>,
    ) -> Result<usize> {
        let cache_dir = self.data_dir.join("cache");
        let lookups_path = cache_dir.join("icon_lookups.json");
        let mut lookups = read_icon_lookups(&lookups_path).unwrap_or_else(|error| {
            // Rewritten below; until then every game is due.
            eprintln!("Steam icons: {error:#}");
            HashMap::new()
        });
        let pending: Vec<WantedIcon> = self
            .games_wanting_icons(now)?
            .into_iter()
            .filter(|game| game.due)
            .collect();
        if pending.is_empty() {
            return Ok(0);
        }
        let appids = crate::install_scanner::steam_appids(&cache_dir);
        if appids.is_empty() {
            // The bundled manifest is empty: no appids until one downloads.
            return Ok(0);
        }

        let mut settled = Vec::new();
        let mut wanted: HashMap<u64, Vec<WantedIcon>> = HashMap::new();
        for game in pending {
            match appids.get(&game.title_normalized) {
                Some(appid) => wanted.entry(*appid).or_default().push(game),
                None => settled.push(game.uuid),
            }
        }
        let mut sorted: Vec<u64> = wanted.keys().copied().collect();
        sorted.sort_unstable();
        let mut hashes = HashMap::new();
        for chunk in sorted.chunks(crate::icon::STEAM_ITEMS_PER_CALL) {
            match crate::icon::steam_community_icons(chunk, &fetch) {
                Ok(found) => {
                    for appid in chunk {
                        match found.get(appid) {
                            Some(hash) => {
                                hashes.insert(*appid, hash.clone());
                            }
                            None => settled.extend(wanted[appid].iter().map(|game| game.uuid)),
                        }
                    }
                }
                Err(error) => {
                    // The rest wait for the next sync, unrecorded.
                    eprintln!("Steam icons: {error:#}");
                    break;
                }
            }
        }

        let mut filled = 0;
        for (appid, hash) in &hashes {
            let png = crate::icon::steam_icon(*appid, hash, &fetch)
                .and_then(|image| crate::icon::normalize(&image));
            // Only Steam saying it has neither size settles the lookup; any
            // other failure (no answer, throttled, refused, a server error, a
            // body that isn't an image) may work on the next sync.
            let png = match png {
                Ok(png) => png,
                Err(error) if crate::icon::is_missing_download(&error) => {
                    settled.extend(wanted[appid].iter().map(|game| game.uuid));
                    continue;
                }
                Err(error) => {
                    eprintln!("Steam icon {appid}: {error:#}");
                    if crate::icon::is_unreachable(&error) {
                        break;
                    }
                    continue;
                }
            };
            let size = crate::icon::dimensions(&png)?;
            for game in &wanted[appid] {
                if game
                    .size
                    .is_some_and(|current| !crate::icon::is_sharper(size, current))
                {
                    settled.push(game.uuid);
                    continue;
                }
                let stored =
                    store::with_lock(self.repository_root(), || self.fill_icon(game, &png));
                match stored {
                    Ok(true) => filled += 1,
                    Ok(false) => {}
                    Err(error) => {
                        eprintln!("Icon for '{}': {error:#}", game.title_normalized);
                        continue;
                    }
                }
                settled.push(game.uuid);
            }
        }

        if !settled.is_empty() {
            lookups.retain(|_, at| now - *at < ICON_LOOKUP_RETRY);
            lookups.extend(settled.into_iter().map(|uuid| (uuid, now)));
            if let Err(error) = write_json(&lookups_path, &lookups) {
                eprintln!("Steam icons: {error:#}");
            }
        }
        if filled > 0 {
            self.notify(Notification::GamesChanged);
        }
        Ok(filled)
    }

    /// `fill_missing_icons`' work, with `fetch` downloading Steam icons.
    /// Icons are read and downloaded outside the repository lock, through
    /// the Add Games preview cache, so an install's icon is only read or
    /// downloaded once; each is stored under the lock by `fill_icon`. A Steam
    /// icon that can't be had right now (offline, refused, a server error)
    /// leaves the game as it is, so a later fill tries again rather than
    /// settling on the client's 32 px copy; one Steam doesn't have falls back
    /// to it. For a game whose automatic icon may still get sharper, a
    /// preview older than `ICON_LOOKUP_RETRY` is made again, so an update
    /// that brings a sharper icon is found without asking every sync.
    fn fill_icons(
        &self,
        installs: &HashMap<String, ScannedInstall>,
        fetch: impl Fn(&str) -> Result<Vec<u8>>,
        now: DateTime<Utc>,
    ) -> Result<usize> {
        let cache = icon_cache_dir(&self.data_dir.join("cache"));
        let mut filled = 0;
        // After one unreachable download, the rest wait for the next fill.
        let mut offline = false;
        for game in self.wanted_install_icons(installs, now)? {
            let Some(source) = game.install.icon() else {
                continue;
            };
            let name = preview_name(&game.install.title);
            let stale_before = game
                .size
                .map(|_| std::time::SystemTime::from(now - ICON_LOOKUP_RETRY));
            let preview = cached_preview(&cache, &name, source, stale_before, &mut offline, &fetch);
            // A preview to retry (Steam failed for now) isn't final.
            let Some(path) = preview.filter(|path| *path == cache.join(format!("{name}.png")))
            else {
                continue;
            };
            let png = match image::open(&path)
                .context("Failed to open the cached icon")
                .and_then(|image| crate::icon::normalize(&image))
            {
                Ok(png) => png,
                Err(error) => {
                    eprintln!("Icon for '{}': {error:#}", game.title_normalized);
                    continue;
                }
            };
            // No sharper than the icon it has: not worth the lock.
            if let (Some(current), Ok(size)) = (game.size, crate::icon::dimensions(&png))
                && !crate::icon::is_sharper(size, current)
            {
                continue;
            }
            let stored = store::with_lock(self.repository_root(), || self.fill_icon(&game, &png));
            match stored {
                Ok(true) => filled += 1,
                Ok(false) => {}
                Err(error) => eprintln!("Icon for '{}': {error:#}", game.title_normalized),
            }
        }
        if filled > 0 {
            self.notify(Notification::GamesChanged);
        }
        Ok(filled)
    }

    /// Stores `png` as `game`'s automatic icon if the game is still here,
    /// still has the title it was matched by, and still takes it: it has no
    /// icon, or an automatic one `png` is sharper than
    /// (`History::takes_auto_icon`). So a user's pick, a rename, a deletion
    /// or a sharper icon that came meanwhile wins, and an icon no sharper
    /// than the stored one writes nothing. The caller holds the repository
    /// lock.
    fn fill_icon(&self, game: &WantedIcon, png: &[u8]) -> Result<bool> {
        let history = self.history();
        let Some(id) = history.index.game_id(game.uuid)? else {
            return Ok(false);
        };
        let slug = history.index.get_game(id)?.game.folder_slug;
        let size = crate::icon::dimensions(png)?;
        let takes = history.store.game_meta(&slug)?.is_some_and(|meta| {
            meta.uuid == game.uuid
                && meta.deleted_at.is_none()
                && store::normalize_display_name(&meta.title) == game.title_normalized
                && history.takes_auto_icon(&meta, size)
        });
        if takes {
            history.set_icon(id, png, store::IconSource::Auto)?;
        }
        Ok(takes)
    }

    /// Sends `notification` while the watcher's notifications run.
    fn notify(&self, notification: Notification) {
        if let Some(notifications) = &*self
            .inner
            .notifications
            .lock()
            .unwrap_or_else(|e| e.into_inner())
        {
            let _ = notifications.send(notification);
        }
    }

    /// At startup, when some game here presses for an icon
    /// (`WantedIcon::is_pressing`), scans installs once: a game installed
    /// since the last scan, or one whose icon couldn't be had then, would
    /// otherwise wait for Add Games or a sync that needs the scan. Runs on
    /// the sync loop's thread before its first sync, which then fills them
    /// when a remote is configured.
    pub(crate) fn scan_for_missing_icons(&self) -> Result<()> {
        if !self
            .games_wanting_icons(Utc::now())?
            .iter()
            .any(WantedIcon::is_pressing)
        {
            return Ok(());
        }
        let candidates = crate::install_scanner::scan_installed_candidates(
            &self.data_dir.join("cache"),
            &self.get_scan_roots()?,
        )?;
        self.remember_installs(ScannedInstall::by_title(&candidates))?;
        self.fill_missing_icons()?;
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
        let problems = self.push_locked(&remote, None)?;
        if !problems.is_empty() {
            bail!("Some games could not be pushed: {}", problems.join("; "));
        }
        Ok(())
    }

    /// Publishes only `games` (after their new versions) and this device's
    /// pointers. The next full sync or push publishes everything else.
    pub fn push_games(&self, games: &HashSet<Uuid>) -> Result<()> {
        let Some(remote) = self.remote()? else {
            return Ok(());
        };
        let _guard = self.sync_lock.lock().unwrap_or_else(|e| e.into_inner());
        let problems = self.push_locked(&remote, Some(games))?;
        if !problems.is_empty() {
            bail!("Some games could not be pushed: {}", problems.join("; "));
        }
        Ok(())
    }

    /// Publishes every game, or only `games` (one game's failure doesn't stop
    /// the others), and this device's pointers. Returns the per-game problems.
    fn push_locked(&self, remote: &Remote, games: Option<&HashSet<Uuid>>) -> Result<Vec<String>> {
        let mut phases = sync::Phases::start("Sync push");
        let history = self.history();
        let state_path = self.data_dir.join("sync_state.json");
        let mut state: SyncState = read_json(&state_path).unwrap_or_default();
        let now = Utc::now();
        // A full heal is left to pushes of every game.
        let full = games.is_none()
            && state
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
        if full {
            // Healing checks every chunk: one listing per shard beats a stat per chunk.
            transfer.list_dst_chunks()?;
        }
        let catalog = match games {
            None => history.store.catalog()?,
            Some(games) => games
                .iter()
                .filter_map(|uuid| history.store.game(*uuid).transpose())
                .collect::<Result<_>>()?,
        };
        phases.lap("prepare");
        let mut problems = Vec::new();
        for game in catalog {
            if let Err(error) = transfer.copy_game(&game, &markers, heal) {
                problems.push(format!("{}: {error:#}", game.meta.title));
            }
        }
        phases.lap(if full { "games (full heal)" } else { "games" });
        let device = self.device_metadata();
        let pointers = DeviceState::from_pointers(device.device_uuid, &history.index.pointers()?);
        let published = remote.publish(&device, &pointers)?;
        sync::save_device(&self.data_dir, &published)?;
        *self.device.write().unwrap_or_else(|e| e.into_inner()) = published;
        phases.lap("pointers");
        if full && problems.is_empty() {
            state.last_full_heal = Some(now);
            write_json(&state_path, &state)?;
        }
        Ok(problems)
    }

    /// Pulls one game; `markers` are the remote's retirement markers.
    fn pull_game(
        &self,
        remote: &Remote,
        game: &GameRecord,
        markers: &HashMap<Uuid, store::RetirementMarker>,
    ) -> Result<Vec<Uuid>> {
        let history = self.history();
        let screenshots = self.get_monitoring_settings()?.sync_screenshots;
        let copy = || {
            let mut transfer = Transfer::new(&remote.store, &history.store, Completeness::Partial)
                .screenshots(screenshots);
            transfer.copy_game(game, markers, Heal::None)?;
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
        let files = history.capture(&live, None)?;
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
        let outcome = self
            .inner
            .checkpoint(&kept, VersionReason::Initial, false)?;
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
        markers: &HashMap<Uuid, store::RetirementMarker>,
        local_uuids: &HashSet<Uuid>,
    ) -> Vec<String> {
        let mut problems = Vec::new();
        for remote_game in remote_games
            .iter()
            .filter(|g| local_uuids.contains(&g.meta.uuid))
        {
            match self.pull_game(remote, remote_game, markers) {
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

    /// Pull, adopt, fast-forward, fill missing icons, push, report.
    pub fn refresh_sync(&self) -> Result<SyncReport> {
        self.refresh_sync_with(crate::icon::download, Utc::now())
    }

    /// `refresh_sync` with `fetch` downloading icons and looking them up on
    /// Steam at `now`.
    fn refresh_sync_with(
        &self,
        fetch: impl Fn(&str) -> Result<Vec<u8>>,
        now: DateTime<Utc>,
    ) -> Result<SyncReport> {
        let mut phases = sync::Phases::start("Full sync");
        let remote = self.require_remote()?;
        let _guard = self.sync_lock.lock().unwrap_or_else(|e| e.into_inner());
        phases.lap("open");
        let mut problems = Vec::new();
        // Read once: nothing below writes the remote catalog before the push.
        let remote_games = remote.store.catalog()?;
        let markers = remote.store.markers()?;
        phases.lap("catalog");
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
                LocalMatch::Collision(_, reason) => {
                    problems.push(format!("{}: {reason}", remote_game.meta.title))
                }
            }
        }

        phases.lap("link");
        let local_uuids = self.local_game_uuids(&untracked)?;
        problems.extend(self.pull_locked(&remote, &remote_games, &markers, &local_uuids));
        phases.lap("pull");

        // Adopt remote games installed here. Scanning is slow, so it runs
        // only once something needs it: adoption, or a fast-forward that
        // must know whether the game is running.
        let unknown: Vec<&GameRecord> = remote_games
            .iter()
            .filter(|g| g.meta.deleted_at.is_none() && !local_uuids.contains(&g.meta.uuid))
            .collect();
        let scan_roots = self.get_scan_roots()?;
        let installs = std::cell::OnceCell::new();
        let installed_titles = || -> &HashMap<String, ScannedInstall> {
            installs.get_or_init(|| {
                ScannedInstall::by_title(
                    &crate::install_scanner::scan_installed_candidates(
                        &self.data_dir.join("cache"),
                        &scan_roots,
                    )
                    .unwrap_or_default(),
                )
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
            match self.adopt(&remote, remote_game, &markers) {
                Ok(true) => {
                    auto_applied.insert(remote_game.meta.uuid);
                }
                Ok(false) => {}
                Err(error) => problems.push(format!("{}: {error:#}", remote_game.meta.title)),
            }
        }
        phases.lap("adopt");

        // Fast-forward slots other devices moved ahead.
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
                    .and_then(|install| install.install_path.clone())
            };
            match self.fast_forward(&remote, remote_game, &game, install) {
                Ok(true) => {
                    auto_applied.insert(remote_game.meta.uuid);
                }
                Ok(false) => {}
                Err(error) => problems.push(format!("{}: {error:#}", game.game.title)),
            }
        }

        phases.lap("fast-forward");
        let scanned = match installs.get() {
            Some(scanned) => Ok(scanned.clone()),
            None => self.installs_for_icons(|| installed_titles().clone(), now),
        };
        match scanned {
            Ok(scanned) => self.fill_icons_before_push(&scanned, fetch, now),
            Err(error) => eprintln!("Icons: {error:#}"),
        }
        phases.lap("icons");
        problems.extend(self.push_locked(&remote, None)?);
        phases.lap("push");
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
        phases.lap("installs");
        let report = self.sync_report(&remote, None, &auto_applied, &installed, problems);
        phases.lap("report");
        report
    }

    /// The sync state as last published, without pulling or pushing.
    pub fn sync_status(&self) -> Result<SyncReport> {
        let remote = self.require_remote()?;
        let installed = self
            .last_installs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        self.sync_report(&remote, None, &HashSet::new(), &installed, Vec::new())
    }

    /// The sync state after a push of `games`: they are read again, the rest
    /// of the remote's games are as the last report read them.
    pub fn pushed_status(&self, games: &HashSet<Uuid>) -> Result<SyncReport> {
        let remote = self.require_remote()?;
        let installed = self
            .last_installs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        self.sync_report(
            &remote,
            Some(games),
            &HashSet::new(),
            &installed,
            Vec::new(),
        )
    }

    /// One game's sync status as it is now (like `sync_status`, without pulling or applying
    /// anything), with the parents of its versions on the remote, so the distance to another
    /// device's tip can be told even before that tip is pulled. `None` when the remote has no
    /// status for the game.
    pub fn game_sync_status(
        &self,
        game_uuid: Uuid,
    ) -> Result<Option<(sync::GameSyncStatus, sync::Parents)>> {
        let remote = self.require_remote()?;
        let installed = self
            .last_installs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let report = self.sync_report(&remote, None, &HashSet::new(), &installed, Vec::new())?;
        let Some(status) = report.games.into_iter().find(|s| s.game_uuid == game_uuid) else {
            return Ok(None);
        };
        let Some(game) = remote.store.game(game_uuid)? else {
            return Ok(None);
        };
        Ok(Some((status, sync::parents(&game))))
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
        if !matches!(found, LocalMatch::Collision(..)) {
            // Adopting is how an untracked game comes back.
            self.history().set_untracked(uuid, false)?;
        }
        match found {
            LocalMatch::None => {
                self.adopt(&remote, &game, &remote.store.markers()?)?;
            }
            LocalMatch::Same(local) => {
                self.link_game(&local, &game)?;
                self.pull_game(&remote, &game, &remote.store.markers()?)?;
            }
            LocalMatch::Collision(kind, reason) => {
                return Err(anyhow::Error::new(kind).context(reason));
            }
        }
        // Pulled, so like a sync it may fill a missing icon before pushing.
        let now = Utc::now();
        let scanned = self.installs_for_icons(
            || {
                let candidates = crate::install_scanner::scan_installed_candidates(
                    &self.data_dir.join("cache"),
                    &self.get_scan_roots().unwrap_or_default(),
                )
                .unwrap_or_default();
                let scanned = ScannedInstall::by_title(&candidates);
                if let Err(error) = self.remember_installs(scanned.clone()) {
                    eprintln!("Install folders: {error:#}");
                }
                scanned
            },
            now,
        );
        match scanned {
            Ok(scanned) => self.fill_icons_before_push(&scanned, crate::icon::download, now),
            Err(error) => eprintln!("Icons: {error:#}"),
        }
        for problem in self.push_locked(&remote, None)? {
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
                LocalMatch::Collision(
                    ErrorKind::NameTaken,
                    format!(
                        "a different local game is also named '{}'; rename one of them to sync it",
                        local.game.title
                    ),
                )
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
            return Ok(LocalMatch::Collision(
                ErrorKind::FolderTaken,
                format!(
                    "its save folder is already tracked by '{}'",
                    local.game.title
                ),
            ));
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
        // The other devices' playthroughs, pointed out until the user finds them.
        let remote_slots: Vec<_> = remote_game
            .slots
            .iter()
            .filter(|slot| slot.meta.deleted_at.is_none() && !slot.versions.is_empty())
            .collect();
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
                meta.icon_source = old.meta.icon_source;
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
            let remote_names: HashSet<String> = remote_slots
                .iter()
                .map(|slot| store::normalize_display_name(&slot.meta.name))
                .collect();
            for slot in &old.slots {
                // Two "Default Slot"s would be told apart only by their dates.
                let mut meta = store::SlotMeta {
                    game_uuid: new_uuid,
                    ..slot.meta.clone()
                };
                if remote_names.contains(&store::normalize_display_name(&meta.name)) {
                    meta.name = format!("{} ({})", meta.name, self.device_metadata().name);
                    meta.updated_at =
                        Utc::now().max(meta.updated_at + chrono::Duration::milliseconds(1));
                }
                history.store.write_slot(&slug, &meta)?;
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
        self.set_linked_slots(
            new_uuid,
            remote_slots.iter().map(|slot| slot.meta.uuid).collect(),
        )
    }

    /// Starts tracking a remote game here. Existing local saves are kept as
    /// a new version; otherwise the newest device's progress is restored.
    /// Returns whether progress was applied.
    fn adopt(
        &self,
        remote: &Remote,
        remote_game: &GameRecord,
        markers: &HashMap<Uuid, store::RetirementMarker>,
    ) -> Result<bool> {
        self.pull_game(remote, remote_game, markers)?;
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
                self.inner
                    .checkpoint(&live, VersionReason::Initial, false)?;
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

    /// The remote's games for a report: all read now, or only `pushed` read
    /// again over the last report's copy, with the last report's statuses.
    fn report_catalog(
        &self,
        remote: &Remote,
        pushed: Option<&HashSet<Uuid>>,
    ) -> Result<(Vec<GameRecord>, Option<Vec<sync::GameSyncStatus>>)> {
        let last = self
            .last_report
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        Ok(match (pushed, last) {
            (Some(pushed), Some(last)) if last.root == remote.store.root() => {
                let mut catalog = last.catalog;
                let history = self.history();
                for &uuid in pushed {
                    let Some(local) = history.store.find_game(uuid)? else {
                        continue;
                    };
                    let fresh = remote.store.game_by_slug(&local.folder_slug)?;
                    let at = catalog.iter().position(|g| g.meta.uuid == uuid);
                    match (fresh, at) {
                        (Some(fresh), Some(at)) => catalog[at] = fresh,
                        (Some(fresh), None) => catalog.push(fresh),
                        (None, Some(at)) => {
                            catalog.remove(at);
                        }
                        (None, None) => {}
                    }
                }
                (catalog, Some(last.statuses))
            }
            _ => (remote.store.catalog()?, None),
        })
    }

    fn sync_report(
        &self,
        remote: &Remote,
        pushed: Option<&HashSet<Uuid>>,
        auto_applied: &HashSet<Uuid>,
        installed_titles: &HashMap<String, ScannedInstall>,
        problems: Vec<String>,
    ) -> Result<SyncReport> {
        let me = self.device_metadata().device_uuid;
        let (catalog, last_statuses) = self.report_catalog(remote, pushed)?;
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
        // Games not pushed keep their last status: their catalog entry is old,
        // so newer tips of other devices would be dropped as not arrived.
        if let (Some(pushed), Some(last)) = (pushed, last_statuses) {
            for status in &mut statuses {
                if let Some(kept) = last
                    .iter()
                    .find(|kept| kept.game_uuid == status.game_uuid)
                    .filter(|_| !pushed.contains(&status.game_uuid))
                {
                    *status = sync::GameSyncStatus {
                        auto_applied: false,
                        ..kept.clone()
                    };
                }
            }
        }
        *self.last_report.lock().unwrap_or_else(|e| e.into_inner()) = Some(LastReport {
            root: remote.store.root().to_path_buf(),
            catalog: catalog.clone(),
            statuses: statuses.clone(),
        });
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
        let mut phases = sync::Phases::start("Pruning");
        let remote = self.remote()?;
        let _guard = self.sync_lock.lock().unwrap_or_else(|e| e.into_inner());
        phases.lap("open");
        if let Some(remote) = &remote {
            if let Some(path) = &remote.unreadable_state {
                bail!(
                    "{} is missing or unreadable; cleanup is skipped until it is fixed",
                    path.display()
                );
            }
            let problems = self.push_locked(remote, None)?;
            if !problems.is_empty() {
                bail!(
                    "Cleanup needs a complete push first: {}",
                    problems.join("; ")
                );
            }
        }
        phases.lap("push");
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
        phases.lap("plan and local cleanup");
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
        phases.lap("remote cleanup");
        history.reindex()?;
        phases.lap("reindex");
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
    Collision(ErrorKind, String),
}

/// What retention planning reads: the union catalog of local and remote,
/// every retired version, and the pointers of all devices and of this one.
struct RetentionInputs {
    games: Vec<GameRecord>,
    retired: HashSet<Uuid>,
    pointed: HashSet<Uuid>,
    local_pointed: HashSet<Uuid>,
}

/// Fills `linked_slot_ids` from the device-local record, leaving out the
/// active slot and slots that no longer exist.
fn with_linked(mut game: FullGame, linked: &HashMap<Uuid, Vec<Uuid>>) -> Result<FullGame> {
    let Some(slots) = linked.get(&game_uuid(&game.game)?) else {
        return Ok(game);
    };
    let active = game.active_slot.as_ref().map(|slot| slot.id);
    game.linked_slot_ids = game
        .slots
        .iter()
        .filter(|slot| Some(slot.id) != active)
        .filter(|slot| slots.iter().any(|uuid| uuid.to_string() == slot.uuid))
        .map(|slot| slot.id)
        .collect();
    Ok(game)
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

/// Pre-release data (a schema predating the current migration series or an
/// older store format) is moved aside. Unknown current/newer schemas and
/// unreadable/newer store metadata stop startup without moving any data.
// Clippy 1.99 flags `QueryableByName`'s expansion of `Row` below.
#[allow(clippy::redundant_field_names)]
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
        let mut old = false;
        for row in applied.iter().filter(|row| !known.contains(&row.version)) {
            if known.iter().all(|version| row.version < *version) {
                old = true;
            } else {
                bail!(
                    "Database migration {} is unsupported by this build; data was left unchanged",
                    row.version
                );
            }
        }
        old
    };
    let metadata = repository.join("metadata.json");
    let old_repository = if metadata.exists() {
        let meta: store::StoreMeta = read_json(&metadata)?;
        if meta.format_version > store::FORMAT_VERSION {
            bail!(
                "Store format {} is unsupported by this build (expected {}); data was left unchanged",
                meta.format_version,
                store::FORMAT_VERSION
            );
        }
        meta.format_version < store::FORMAT_VERSION
    } else {
        false
    };
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

/// The Add Games preview of `source`, cached in `dir` as `{name}.png` once
/// final. When a Steam download fails in a way a later scan may get past
/// (offline, a server error, a body that isn't an image), the local fallback
/// goes to `{name}.retry.png` instead, so the next scan tries again; a
/// missing icon is final. Once Steam proves unreachable, `offline` makes the
/// rest of the scan skip downloads instead of waiting out a timeout per game.
/// A final preview written before `stale_before` is made again.
fn cached_preview(
    dir: &Path,
    name: &str,
    source: crate::icon::Source,
    stale_before: Option<std::time::SystemTime>,
    offline: &mut bool,
    fetch: impl FnOnce(&str) -> Result<Vec<u8>>,
) -> Option<PathBuf> {
    let settled = dir.join(format!("{name}.png"));
    let fresh = match stale_before {
        None => settled.exists(),
        Some(before) => std::fs::metadata(&settled)
            .and_then(|meta| meta.modified())
            .is_ok_and(|at| at >= before),
    };
    if fresh {
        return Some(settled);
    }
    let skip = *offline;
    let (image, failed) = crate::icon::resolve_with(source, |url| {
        if skip {
            bail!("Skipped while offline");
        }
        fetch(url)
    })
    .ok()?;
    let retry = dir.join(format!("{name}.retry.png"));
    let path = match &failed {
        Some(error) if !crate::icon::is_missing_download(error) => {
            *offline |= crate::icon::is_unreachable(error);
            retry
        }
        _ => {
            let _ = std::fs::remove_file(&retry);
            settled
        }
    };
    image.save(&path).ok()?;
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A core tracking one game whose saves changed after its first version.
    fn changed_game() -> (WorldlineCore, i32, tempfile::TempDir, tempfile::TempDir) {
        let data = tempfile::tempdir().unwrap();
        let saves = tempfile::tempdir().unwrap();
        let (core, _) = WorldlineCore::open(data.path(), OpenOptions::default()).unwrap();
        std::fs::write(saves.path().join("save.dat"), "level 1").unwrap();
        let game = core
            .save_game(
                GameForm {
                    id: None,
                    title: "Hollow Knight".into(),
                },
                Some(vec![saves.path().to_string_lossy().into_owned()]),
            )
            .unwrap();
        std::fs::write(saves.path().join("save.dat"), "level 2").unwrap();
        (core, game.id, data, saves)
    }

    /// The created version and the active one, as the app's views read them.
    fn created_and_active(core: &WorldlineCore, game_id: i32, kept: &Kept) -> (Version, Version) {
        let CheckpointOutcome::Created(created) = &kept.checkpoint else {
            panic!("a created version: {kept:?}");
        };
        let active = core.get_game(game_id).unwrap().last_version.unwrap();
        (core.get_version(created.id).unwrap().version, active)
    }

    #[test]
    fn live_saves_differ_by_content_as_a_checkpoint_would_see_them() {
        let (core, game_id, _data, saves) = changed_game();
        let differs = || {
            core.history()
                .live_differs(&core.get_game(game_id).unwrap())
                .unwrap()
        };
        let save = saves.path().join("save.dat");
        assert!(differs(), "content changed");
        core.checkpoint(game_id).unwrap();
        assert!(!differs(), "unchanged");
        // The same bytes, written later.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&save, "level 2").unwrap();
        assert!(!differs(), "same bytes rewritten");
        std::fs::write(saves.path().join("output.log"), "noise").unwrap();
        assert!(!differs(), "an excluded file");
        std::fs::write(saves.path().join("extra.dat"), "new").unwrap();
        assert!(differs(), "a file added");
        core.checkpoint(game_id).unwrap();
        std::fs::remove_file(&save).unwrap();
        assert!(differs(), "a file removed");
    }

    #[test]
    fn saves_grown_past_the_limit_differ_without_being_read() {
        let (core, game_id, _data, saves) = changed_game();
        for n in 0..crate::history::MIN_CAPTURE_LIMIT.files {
            std::fs::write(saves.path().join(format!("{n}.dat")), "").unwrap();
        }
        let game = core.get_game(game_id).unwrap();
        assert!(core.history().live_differs(&game).unwrap());
    }

    #[test]
    fn an_empty_game_without_versions_has_nothing_unrecorded() {
        let data = tempfile::tempdir().unwrap();
        let saves = tempfile::tempdir().unwrap();
        let (core, _) = WorldlineCore::open(data.path(), OpenOptions::default()).unwrap();
        let game = core
            .save_game(
                GameForm {
                    id: None,
                    title: "Empty".into(),
                },
                Some(vec![saves.path().to_string_lossy().into_owned()]),
            )
            .unwrap();
        let game = core.get_game(game.id).unwrap();
        assert!(game.last_version.is_none());
        assert!(!core.history().live_differs(&game).unwrap());
    }

    #[test]
    fn a_tag_that_fails_to_publish_still_reports_the_created_version() {
        let (core, game_id, _data, _saves) = changed_game();
        let kept = core
            .keep_with(
                game_id,
                "Kept in game, 21:04",
                || true,
                |_, _| bail!("disk full"),
            )
            .unwrap()
            .unwrap();
        assert!(matches!(kept.tagged, Some(Err(_))));
        let (created, active) = created_and_active(&core, game_id, &kept);
        assert_eq!(active.id, created.id);
        assert_eq!(created.tag, None);
        assert_eq!(core.get_game(game_id).unwrap().versions.len(), 2);
    }

    #[test]
    fn a_tag_written_but_not_indexed_shows_what_the_files_say() {
        let (core, game_id, _data, _saves) = changed_game();
        let kept = core
            .keep_with(
                game_id,
                "Kept in game, 21:04",
                || true,
                |history, id| {
                    let version = history.index.get_version(id)?;
                    let mut meta = history.version_meta(&version)?;
                    meta.tag = Some("Kept in game, 21:04".into());
                    // Newer metadata wins, as `tag_version` writes it.
                    meta.updated_at += chrono::Duration::milliseconds(1);
                    history.store.publish_version(
                        &version.game.folder_slug,
                        &version.slot.folder_slug,
                        &meta,
                    )?;
                    bail!("index refresh failed")
                },
            )
            .unwrap()
            .unwrap();
        assert!(matches!(kept.tagged, Some(Err(_))));
        let (created, active) = created_and_active(&core, game_id, &kept);
        assert_eq!(active.id, created.id);
        assert_eq!(created.tag.as_deref(), Some("Kept in game, 21:04"));
    }

    fn jpg(size: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgb8(image::RgbImage::new(size, size))
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Jpeg,
            )
            .unwrap();
        bytes
    }

    fn http_status(status: u16) -> anyhow::Error {
        ureq::Error::Status(status, ureq::Response::new(status, "", "").unwrap()).into()
    }

    /// A temp preview cache and a Steam game's local 32 px icon.
    fn steam_preview_dirs() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let local = dir
            .path()
            .join("3eec57d783c54036166564e195039b20f600a52f.jpg");
        std::fs::write(&local, jpg(32)).unwrap();
        std::fs::create_dir(dir.path().join("cache")).unwrap();
        (dir, local)
    }

    fn preview(
        dir: &Path,
        local: &Path,
        offline: &mut bool,
        fetch: impl FnOnce(&str) -> Result<Vec<u8>>,
    ) -> (PathBuf, u32) {
        let source = crate::icon::Source::Steam { appid: 1, local };
        let path =
            cached_preview(&dir.join("cache"), "Game", source, None, offline, fetch).unwrap();
        let width = image::open(&path).unwrap().width();
        (path.strip_prefix(dir).unwrap().to_owned(), width)
    }

    #[test]
    fn a_preview_whose_steam_download_failed_for_now_is_retried_next_scan() {
        type Fetch = Box<dyn FnOnce(&str) -> Result<Vec<u8>>>;
        let failures: [(Fetch, bool); 3] = [
            (Box::new(|_| Err(http_status(503))), true),
            // Forbidden for now, not missing.
            (Box::new(|_| Err(http_status(403))), false),
            // An HTTP 200 whose body isn't an image.
            (Box::new(|_| Ok(b"<html>".to_vec())), false),
        ];
        for (fetch, unreachable) in failures {
            let (dir, local) = steam_preview_dirs();
            let dir = dir.path();
            let mut offline = false;
            let fallback = preview(dir, &local, &mut offline, fetch);
            assert_eq!(fallback, (Path::new("cache/Game.retry.png").into(), 32));
            assert_eq!(offline, unreachable);
            assert!(!dir.join("cache/Game.png").exists());

            // The next scan.
            let mut offline = false;
            let upgraded = preview(dir, &local, &mut offline, |_| Ok(jpg(184)));
            assert_eq!(upgraded, (Path::new("cache/Game.png").into(), 184));
            assert!(!dir.join("cache/Game.retry.png").exists());
            let cached = preview(dir, &local, &mut offline, |_| panic!("cached"));
            assert_eq!(cached, upgraded);
        }
    }

    #[test]
    fn a_missing_steam_icon_settles_on_the_local_one() {
        let (dir, local) = steam_preview_dirs();
        let dir = dir.path();
        let mut offline = false;
        let fallback = preview(dir, &local, &mut offline, |_| Err(http_status(404)));
        assert_eq!(fallback, (Path::new("cache/Game.png").into(), 32));
        assert!(!offline);
        let cached = preview(dir, &local, &mut offline, |_| panic!("cached"));
        assert_eq!(cached, fallback);
    }

    /// What a scan found for `changed_game`'s game: a Steam install with its local icon.
    fn steam_install(local: &Path) -> HashMap<String, ScannedInstall> {
        HashMap::from([(
            store::normalize_display_name("Hollow Knight"),
            ScannedInstall {
                title: "Hollow Knight".into(),
                install_path: None,
                icon_path: Some(local.to_string_lossy().into_owned()),
                steam_appid: Some(1),
            },
        )])
    }

    fn icon_sha(core: &WorldlineCore, game_id: i32) -> Option<String> {
        let slug = core.get_game(game_id).unwrap().game.folder_slug;
        core.history().store.game_meta(&slug).unwrap()?.icon_sha256
    }

    #[test]
    fn a_missing_icon_steam_cant_send_for_now_is_filled_by_a_later_try() {
        let (core, id, _data, _saves) = changed_game();
        let (_dir, local) = steam_preview_dirs();
        let installs = steam_install(&local);
        for status in [503, 429, 403] {
            let filled = core.fill_icons(&installs, |_| Err(http_status(status)), Utc::now());
            assert_eq!(filled.unwrap(), 0, "{status}: not the 32 px copy");
            assert_eq!(icon_sha(&core, id), None);
        }
        assert_eq!(
            core.fill_icons(&installs, |_| Ok(jpg(184)), Utc::now())
                .unwrap(),
            1
        );
        assert!(icon_sha(&core, id).is_some());
        let again = core.fill_icons(&installs, |_| panic!("already has one"), Utc::now());
        assert_eq!(again.unwrap(), 0);
    }

    #[test]
    fn an_icon_steam_doesnt_have_falls_back_to_the_local_one() {
        let (core, id, _data, _saves) = changed_game();
        let (_dir, local) = steam_preview_dirs();
        let filled = core.fill_icons(
            &steam_install(&local),
            |_| Err(http_status(404)),
            Utc::now(),
        );
        assert_eq!(filled.unwrap(), 1);
        assert!(icon_sha(&core, id).is_some());
        assert_eq!(game_meta(&core, id).icon_source, store::IconSource::Auto);
    }

    /// The download runs outside the repository lock (these changes take it,
    /// so they would wait forever otherwise), and the fill only stores the
    /// icon if none of them happened meanwhile.
    #[test]
    fn a_fill_never_overwrites_what_changed_while_the_icon_downloaded() {
        let user_icon = tempfile::tempdir().unwrap();
        let picked = user_icon.path().join("picked.png");
        image::RgbaImage::from_pixel(48, 48, image::Rgba([255, 0, 0, 255]))
            .save(&picked)
            .unwrap();
        type Change<'a> = Box<dyn Fn(&WorldlineCore, i32) + 'a>;
        let changes: [(&str, Change); 3] = [
            (
                "a user's icon",
                Box::new(|core, id| {
                    core.set_game_icon(id, &picked.to_string_lossy()).unwrap();
                }),
            ),
            (
                "a rename",
                Box::new(|core, id| {
                    let form = GameForm {
                        id: Some(id),
                        title: "Silksong".into(),
                    };
                    core.save_game(form, None).unwrap();
                }),
            ),
            (
                "a deletion",
                Box::new(|core, id| core.delete_game(id).unwrap()),
            ),
        ];
        for (change, apply) in changes {
            let (core, id, _data, _saves) = changed_game();
            let uuid = core.get_game(id).unwrap().game.uuid;
            let (_dir, local) = steam_preview_dirs();
            let filled = core.fill_icons(
                &steam_install(&local),
                |_| {
                    apply(&core, id);
                    Ok(jpg(184))
                },
                Utc::now(),
            );
            assert_eq!(filled.unwrap(), 0, "{change}");
            let history = core.history();
            let meta = history
                .store
                .game_metas()
                .unwrap()
                .into_iter()
                .find(|meta| meta.uuid.to_string() == uuid)
                .unwrap();
            if change == "a user's icon" {
                let png = std::fs::read(history.store.icon_path(&meta.folder_slug)).unwrap();
                assert_eq!(meta.icon_sha256, Some(crate::icon::sha256(&png)));
                let stored = crate::icon::decode(&png).unwrap().to_rgba8();
                assert_eq!(stored.get_pixel(0, 0).0, [255, 0, 0, 255], "the user's");
            } else {
                assert_eq!(meta.icon_sha256, None, "{change}");
            }
            if change == "a deletion" {
                assert!(meta.deleted_at.is_some(), "not brought back");
                assert_eq!(history.index.game_id(meta.uuid).unwrap(), None);
            }
        }
    }

    #[test]
    fn once_steam_is_unreachable_the_rest_of_the_scan_skips_downloads() {
        let (dir, local) = steam_preview_dirs();
        let mut offline = true;
        let fallback = preview(dir.path(), &local, &mut offline, |_| {
            panic!("skipped while offline")
        });
        assert_eq!(fallback, (Path::new("cache/Game.retry.png").into(), 32));
    }

    // ---- Steam icons for games not installed here (phase 3) ----------------------

    const HASH_A: &str = "3eec57d783c54036166564e195039b20f600a52f";
    const HASH_B: &str = "e2cae122ca4663db2108808148d2793eef2f3e0f";

    /// A core tracking `titles`, none installed, with a manifest naming
    /// `appids` by title.
    fn steam_games(
        titles: &[&str],
        appids: &[(&str, u64)],
    ) -> (
        WorldlineCore,
        Vec<i32>,
        tempfile::TempDir,
        tempfile::TempDir,
    ) {
        let data = tempfile::tempdir().unwrap();
        let saves = tempfile::tempdir().unwrap();
        write_manifest(data.path(), appids);
        let (core, _) = WorldlineCore::open(data.path(), OpenOptions::default()).unwrap();
        let ids = titles
            .iter()
            .enumerate()
            .map(|(n, title)| {
                let dir = saves.path().join(n.to_string());
                std::fs::create_dir_all(&dir).unwrap();
                let form = GameForm {
                    id: None,
                    title: title.to_string(),
                };
                let dirs = vec![dir.to_string_lossy().into_owned()];
                core.save_game(form, Some(dirs)).unwrap().id
            })
            .collect();
        (core, ids, data, saves)
    }

    /// A fresh manifest cache, so `load_manifest` never downloads one.
    fn write_manifest(data: &Path, appids: &[(&str, u64)]) {
        let yaml: String = appids
            .iter()
            .map(|(title, appid)| format!("{title}:\n  steam: {{id: {appid}}}\n"))
            .collect();
        let cache = data.join("cache");
        std::fs::create_dir_all(&cache).unwrap();
        let yaml = if yaml.is_empty() { "{}".into() } else { yaml };
        std::fs::write(cache.join("manifest.yaml"), yaml).unwrap();
    }

    /// A `GetItems` answer: an icon hash per appid, or `success: 15` for `None`.
    fn get_items(items: &[(u64, Option<&str>)]) -> Vec<u8> {
        let items: Vec<_> = items
            .iter()
            .map(|(appid, hash)| match hash {
                Some(hash) => serde_json::json!({
                    "item_type": 0, "id": appid, "success": 1, "appid": appid,
                    "assets": { "community_icon": hash, "header": "header.jpg" },
                }),
                None => serde_json::json!({ "item_type": 0, "id": appid, "success": 15 }),
            })
            .collect();
        serde_json::to_vec(&serde_json::json!({ "response": { "store_items": items } })).unwrap()
    }

    /// A fake Steam logging every URL asked for: `GetItems` answers `items`,
    /// `_full` icons `full()`, and 32 px icons a 32 px JPG.
    fn fake_steam<'a>(
        log: &'a std::cell::RefCell<Vec<String>>,
        items: &'a [u8],
        full: impl Fn() -> Result<Vec<u8>> + 'a,
    ) -> impl Fn(&str) -> Result<Vec<u8>> + 'a {
        move |url| {
            log.borrow_mut().push(url.to_string());
            if url.contains("/IStoreBrowseService/GetItems/v1/") {
                Ok(items.to_vec())
            } else if url.ends_with("_full.jpg") {
                full()
            } else {
                Ok(jpg(32))
            }
        }
    }

    fn lookups(data: &Path) -> HashMap<Uuid, DateTime<Utc>> {
        read_json(&data.join("cache").join("icon_lookups.json")).unwrap_or_default()
    }

    fn uuid_of(core: &WorldlineCore, id: i32) -> Uuid {
        Uuid::parse_str(&core.get_game(id).unwrap().game.uuid).unwrap()
    }

    fn at(day: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000, 0).unwrap() + chrono::Duration::days(day)
    }

    fn user_icon(dir: &Path) -> String {
        let picked = dir.join("picked.png");
        image::RgbaImage::from_pixel(48, 48, image::Rgba([255, 0, 0, 255]))
            .save(&picked)
            .unwrap();
        picked.to_string_lossy().into_owned()
    }

    fn stored_pixel(core: &WorldlineCore, id: i32) -> [u8; 4] {
        let slug = core.get_game(id).unwrap().game.folder_slug;
        let png = std::fs::read(core.history().store.icon_path(&slug)).unwrap();
        crate::icon::decode(&png)
            .unwrap()
            .to_rgba8()
            .get_pixel(0, 0)
            .0
    }

    #[test]
    fn steam_icons_are_looked_up_in_one_call_and_misses_are_remembered() {
        let (core, ids, data, _saves) = steam_games(
            &["Celeste", "Doom", "Delisted", "Homebrew"],
            &[("Celeste", 504230), ("Doom", 2280), ("Delisted", 1024020)],
        );
        let log = Default::default();
        let items = get_items(&[
            (504230, Some(HASH_A)),
            (2280, Some(HASH_B)),
            (1024020, None),
        ]);
        let filled = core.fill_steam_icons(fake_steam(&log, &items, || Ok(jpg(184))), at(0));
        assert_eq!(filled.unwrap(), 2);

        let log = log.into_inner();
        let calls: Vec<_> = log.iter().filter(|url| url.contains("GetItems")).collect();
        assert_eq!(calls.len(), 1, "one batched call: {log:?}");
        for appid in [504230, 2280, 1024020] {
            assert!(calls[0].contains(&format!("%7B%22appid%22%3A{appid}%7D")));
        }
        assert!(calls[0].contains("include_assets%22%3Atrue"));
        assert!(log.contains(&format!(
            "https://cdn.cloudflare.steamstatic.com/steamcommunity/public/images/apps/504230/{HASH_A}_full.jpg"
        )));
        assert!(icon_sha(&core, ids[0]).is_some());
        assert!(icon_sha(&core, ids[1]).is_some());
        assert_eq!(
            game_meta(&core, ids[0]).icon_source,
            store::IconSource::Auto
        );
        assert_eq!(icon_sha(&core, ids[2]), None);
        assert_eq!(icon_sha(&core, ids[3]), None);
        // Every answer is remembered: delisted (`success: 15`), not on
        // Steam, and hits, whose 184 px icons Steam won't sharpen next sync.
        assert_eq!(
            lookups(data.path()),
            ids.iter().map(|id| (uuid_of(&core, *id), at(0))).collect()
        );
    }

    #[test]
    fn appids_past_one_call_are_asked_in_chunks() {
        let titles: Vec<String> = (0..=crate::icon::STEAM_ITEMS_PER_CALL)
            .map(|n| format!("Game {n}"))
            .collect();
        let appids: Vec<(&str, u64)> = titles
            .iter()
            .enumerate()
            .map(|(n, title)| (title.as_str(), n as u64 + 1))
            .collect();
        let refs: Vec<&str> = titles.iter().map(String::as_str).collect();
        let (core, _ids, data, _saves) = steam_games(&refs, &appids);
        let calls = std::cell::Cell::new(0);
        let steam = |_: &str| {
            calls.set(calls.get() + 1);
            Ok(get_items(&[]))
        };
        assert_eq!(core.fill_steam_icons(steam, at(0)).unwrap(), 0);
        assert_eq!(calls.get(), 2);
        assert_eq!(lookups(data.path()).len(), titles.len());
    }

    #[test]
    fn a_miss_is_looked_up_again_after_seven_days() {
        let (core, ids, _data, _saves) = steam_games(&["Delisted"], &[("Delisted", 1024020)]);
        let log = Default::default();
        let miss = get_items(&[(1024020, None)]);
        let steam = fake_steam(&log, &miss, || panic!("no icon to download"));
        assert_eq!(core.fill_steam_icons(&steam, at(0)).unwrap(), 0);
        assert_eq!(log.borrow().len(), 1);

        let just_before = at(7) - chrono::Duration::seconds(1);
        assert_eq!(core.fill_steam_icons(&steam, just_before).unwrap(), 0);
        assert_eq!(log.borrow().len(), 1, "within the week: not asked again");

        let hit = get_items(&[(1024020, Some(HASH_A))]);
        let steam = fake_steam(&log, &hit, || Ok(jpg(184)));
        assert_eq!(core.fill_steam_icons(&steam, at(7)).unwrap(), 1);
        assert!(icon_sha(&core, ids[0]).is_some());
        let steam = fake_steam(&log, &hit, || panic!("already has one"));
        assert_eq!(core.fill_steam_icons(&steam, at(8)).unwrap(), 0);
    }

    #[test]
    fn without_a_full_icon_the_32_px_one_is_used() {
        let (core, ids, data, _saves) = steam_games(&["Old Game"], &[("Old Game", 70)]);
        let log = Default::default();
        let items = get_items(&[(70, Some(HASH_A))]);
        let steam = fake_steam(&log, &items, || Err(http_status(404)));
        assert_eq!(core.fill_steam_icons(steam, at(0)).unwrap(), 1);
        assert!(icon_sha(&core, ids[0]).is_some());
        let base = "https://cdn.cloudflare.steamstatic.com/steamcommunity/public/images/apps/70";
        assert_eq!(
            log.into_inner()[1..],
            [
                format!("{base}/{HASH_A}_full.jpg"),
                format!("{base}/{HASH_A}.jpg")
            ]
        );
        assert_eq!(lookups(data.path()).len(), 1, "not asked again next sync");
    }

    #[test]
    fn an_icon_steam_has_in_neither_size_is_a_miss() {
        let (core, ids, data, _saves) = steam_games(&["Old Game"], &[("Old Game", 70)]);
        let items = get_items(&[(70, Some(HASH_A))]);
        let steam = |url: &str| {
            if url.contains("GetItems") {
                Ok(items.clone())
            } else {
                Err(http_status(404))
            }
        };
        assert_eq!(core.fill_steam_icons(steam, at(0)).unwrap(), 0);
        assert_eq!(icon_sha(&core, ids[0]), None);
        assert_eq!(lookups(data.path()).len(), 1);
    }

    /// Failures that may pass leave nothing behind, so the next sync tries
    /// again, and never become an error.
    #[test]
    fn a_lookup_or_download_that_failed_for_now_is_not_remembered() {
        type Fetch = Box<dyn Fn(&str) -> Result<Vec<u8>>>;
        let items = get_items(&[(504230, Some(HASH_A)), (2280, Some(HASH_B))]);
        let answer = |full: u16| -> Fetch {
            let items = items.clone();
            Box::new(move |url: &str| {
                if url.contains("GetItems") {
                    Ok(items.clone())
                } else {
                    Err(http_status(full))
                }
            })
        };
        // A transport error, as when offline: nothing listens there.
        let refused = ureq::get("http://127.0.0.1:9/")
            .timeout(std::time::Duration::from_secs(2))
            .call()
            .unwrap_err();
        assert!(matches!(refused, ureq::Error::Transport(_)));
        let refused = std::cell::RefCell::new(Some(refused));
        let offline: Fetch = Box::new(move |_| Err(refused.borrow_mut().take().unwrap().into()));
        let failures: [(&str, Fetch); 6] = [
            ("offline", offline),
            ("throttled", Box::new(|_| Err(http_status(429)))),
            ("not JSON", Box::new(|_| Ok(b"<html>".to_vec()))),
            (
                "not Steam's JSON",
                Box::new(|_| Ok(br#"{"ok":true}"#.to_vec())),
            ),
            ("icon refused", answer(403)),
            ("CDN down", answer(503)),
        ];
        let downloads = std::cell::Cell::new(0);
        for (failure, fetch) in failures {
            let (core, ids, data, _saves) =
                steam_games(&["Celeste", "Doom"], &[("Celeste", 504230), ("Doom", 2280)]);
            let counted = |url: &str| {
                if !url.contains("GetItems") {
                    downloads.set(downloads.get() + 1);
                }
                fetch(url)
            };
            downloads.set(0);
            let filled = core.fill_steam_icons(counted, at(0));
            assert_eq!(filled.unwrap(), 0, "{failure}");
            assert_eq!(icon_sha(&core, ids[0]), None, "{failure}");
            assert!(lookups(data.path()).is_empty(), "{failure}");
            match failure {
                "icon refused" => assert_eq!(downloads.get(), 2, "each tried"),
                "CDN down" => assert_eq!(downloads.get(), 1, "the rest wait"),
                _ => {}
            }
        }
    }

    /// The CDN answering 200 with something that isn't an image (a portal's
    /// page, a damaged file), for the 184 px icon or the 32 px fallback, is a
    /// failure for now: not remembered, so the next sync, days before the
    /// week is out, fills or sharpens the icon.
    #[test]
    fn an_icon_download_that_isnt_an_image_is_tried_again_next_sync() {
        let items = get_items(&[(504230, Some(HASH_A))]);
        let page = || -> Result<Vec<u8>> { Ok(b"<html>Sign in to this network</html>".to_vec()) };
        let damaged = || -> Result<Vec<u8>> { Ok(jpg(184)[..64].to_vec()) };
        for broken in [
            "184 px page",
            "184 px damaged",
            "32 px page",
            "32 px damaged",
        ] {
            for small in [false, true] {
                let case = format!("{broken}, small icon before: {small}");
                let (core, ids, data, saves) = steam_games(&["Celeste"], &[("Celeste", 504230)]);
                let id = ids[0];
                if small {
                    core.set_game_icon_as(
                        id,
                        &png_file(saves.path(), 32, RED),
                        store::IconSource::Auto,
                    )
                    .unwrap();
                }
                let before = game_meta(&core, id);
                let bad = if broken.contains("page") {
                    page
                } else {
                    damaged
                };
                let steam = |url: &str| {
                    if url.contains("GetItems") {
                        Ok(items.clone())
                    } else if broken.starts_with("184") || !url.ends_with("_full.jpg") {
                        bad()
                    } else {
                        Err(http_status(404))
                    }
                };
                assert_eq!(core.fill_steam_icons(steam, at(0)).unwrap(), 0, "{case}");
                assert_eq!(game_meta(&core, id), before, "{case}");
                assert!(lookups(data.path()).is_empty(), "{case}: not a miss");

                let log = Default::default();
                let fixed = fake_steam(&log, &items, || Ok(jpg(184)));
                assert_eq!(core.fill_steam_icons(fixed, at(1)).unwrap(), 1, "{case}");
                assert_eq!(stored_size(&core, id), (184, 184), "{case}");
                assert_eq!(game_meta(&core, id).icon_source, store::IconSource::Auto);
                assert_eq!(lookups(data.path())[&uuid_of(&core, id)], at(1), "{case}");
            }
        }
    }

    #[test]
    fn only_valid_hashes_for_the_appids_asked_about_become_urls() {
        let (core, ids, data, _saves) = steam_games(&["Celeste"], &[("Celeste", 504230)]);
        let answers = [
            get_items(&[(504230, Some("../../../evil"))]),
            get_items(&[(504230, Some("ZZec57d783c54036166564e195039b20f600a52f"))]),
            // An item for an appid nobody asked about.
            get_items(&[(666, Some(HASH_A))]),
        ];
        for items in answers {
            let log = std::cell::RefCell::new(Vec::new());
            let steam = fake_steam(&log, &items, || panic!("no URL from that answer"));
            assert_eq!(core.fill_steam_icons(steam, at(0)).unwrap(), 0);
            assert_eq!(log.borrow().len(), 1);
            assert_eq!(icon_sha(&core, ids[0]), None);
            std::fs::remove_file(data.path().join("cache").join("icon_lookups.json")).unwrap();
        }
    }

    #[test]
    fn games_with_an_icon_or_without_a_manifest_ask_steam_nothing() {
        let (core, ids, _data, saves) = steam_games(&["Celeste"], &[("Celeste", 504230)]);
        core.set_game_icon(ids[0], &user_icon(saves.path()))
            .unwrap();
        let filled = core.fill_steam_icons(|_| panic!("has an icon"), at(0));
        assert_eq!(filled.unwrap(), 0);
        assert_eq!(stored_pixel(&core, ids[0]), [255, 0, 0, 255]);

        // Only the bundled (empty) manifest: nothing to look up by, and no
        // game is marked as a miss for it.
        let (core, _ids, data, _saves) = steam_games(&["Celeste"], &[]);
        let filled = core.fill_steam_icons(|_| panic!("no appids"), at(0));
        assert_eq!(filled.unwrap(), 0);
        assert!(lookups(data.path()).is_empty());
    }

    #[test]
    fn an_unreadable_lookup_cache_starts_over() {
        let (core, ids, data, _saves) = steam_games(&["Celeste"], &[("Celeste", 504230)]);
        let path = data.path().join("cache").join("icon_lookups.json");
        std::fs::write(&path, "{ not json").unwrap();
        let miss = get_items(&[(504230, None)]);
        let log = Default::default();
        let steam = fake_steam(&log, &miss, || panic!("a miss"));
        assert_eq!(core.fill_steam_icons(&steam, at(0)).unwrap(), 0);
        assert_eq!(
            lookups(data.path()),
            HashMap::from([(uuid_of(&core, ids[0]), at(0))])
        );
    }

    #[test]
    fn a_steam_fill_never_overwrites_what_changed_while_the_icon_downloaded() {
        let items = get_items(&[(504230, Some(HASH_A))]);
        let log = Default::default();

        let (core, ids, _data, saves) = steam_games(&["Celeste"], &[("Celeste", 504230)]);
        let picked = user_icon(saves.path());
        let steam = fake_steam(&log, &items, || {
            core.set_game_icon(ids[0], &picked).unwrap();
            Ok(jpg(184))
        });
        assert_eq!(core.fill_steam_icons(steam, at(0)).unwrap(), 0);
        assert_eq!(stored_pixel(&core, ids[0]), [255, 0, 0, 255], "the user's");

        let (core, ids, _data, _saves) = steam_games(&["Celeste"], &[("Celeste", 504230)]);
        let steam = fake_steam(&log, &items, || {
            let form = GameForm {
                id: Some(ids[0]),
                title: "Celeste Classic".into(),
            };
            core.save_game(form, None).unwrap();
            Ok(jpg(184))
        });
        assert_eq!(core.fill_steam_icons(steam, at(0)).unwrap(), 0, "renamed");
        assert_eq!(icon_sha(&core, ids[0]), None);

        let (core, ids, _data, _saves) = steam_games(&["Celeste"], &[("Celeste", 504230)]);
        let steam = fake_steam(&log, &items, || {
            core.delete_game(ids[0]).unwrap();
            Ok(jpg(184))
        });
        assert_eq!(core.fill_steam_icons(steam, at(0)).unwrap(), 0, "deleted");
    }

    /// What a scan found for Celeste: an install whose own icon is a blue
    /// `size` px image.
    fn own_install(dir: &Path, size: u32) -> HashMap<String, ScannedInstall> {
        let own = dir.join(format!("celeste-{size}.png"));
        image::RgbaImage::from_pixel(size, size, image::Rgba([0, 0, 255, 255]))
            .save(&own)
            .unwrap();
        HashMap::from([(
            "celeste".to_string(),
            ScannedInstall {
                title: "Celeste".into(),
                install_path: None,
                icon_path: Some(own.to_string_lossy().into_owned()),
                steam_appid: None,
            },
        )])
    }

    #[test]
    fn an_installed_games_own_icon_wins_without_asking_steam() {
        let (core, ids, _data, saves) = steam_games(&["Celeste"], &[("Celeste", 504230)]);
        let installs = own_install(saves.path(), 256);
        core.fill_icons_before_push(&installs, |url| panic!("asked Steam: {url}"), at(0));
        assert_eq!(stored_pixel(&core, ids[0]), [0, 0, 255, 255]);
    }

    #[test]
    fn installs_are_scanned_for_icons_once_and_only_when_some_game_lacks_one() {
        let (core, _ids, _data, _saves) = steam_games(&["Celeste"], &[]);
        let scans = std::cell::Cell::new(0);
        let scan = || {
            scans.set(scans.get() + 1);
            HashMap::new()
        };
        core.installs_for_icons(scan, at(0)).unwrap();
        assert_eq!(scans.get(), 1, "lacks an icon, nothing scanned yet");
        core.remember_installs(HashMap::new()).unwrap();
        core.installs_for_icons(scan, at(0)).unwrap();
        assert_eq!(scans.get(), 1, "this process scanned already");

        let (core, ids, _data, saves) = steam_games(&["Celeste"], &[]);
        core.set_game_icon(ids[0], &user_icon(saves.path()))
            .unwrap();
        core.installs_for_icons(|| panic!("every game has an icon"), at(0))
            .unwrap();
    }

    /// The icon is looked up after the pull, by the title the pull brought
    /// in, and reaches the other device with the push.
    #[test]
    fn a_steam_icon_is_looked_up_after_the_pull_and_reaches_the_other_device() {
        let remote = tempfile::tempdir().unwrap();
        // Made-up appids, so no Steam install on this machine matches them.
        let appids = [("Ember Trail", 900000001), ("Ember Trail II", 900000002)];
        let synced = |appids: &[(&str, u64)]| {
            let (core, _, data, saves) = steam_games(&[], appids);
            let settings = SyncSettings {
                remote_path: Some(remote.path().to_string_lossy().into_owned()),
            };
            core.update_sync_settings(settings).unwrap();
            (core, data, saves)
        };
        let (a, _a_data, a_saves) = synced(&appids);
        // B adopts it before it has a manifest, so without looking it up.
        let (b, b_data, _b_saves) = synced(&[]);
        let form = GameForm {
            id: None,
            title: "Ember Trail".into(),
        };
        let dirs = vec![a_saves.path().to_string_lossy().into_owned()];
        let id = a.save_game(form, Some(dirs)).unwrap().id;
        let uuid = uuid_of(&a, id);
        let offline = |_: &str| -> Result<Vec<u8>> { Err(http_status(503)) };
        a.refresh_sync_with(offline, at(0)).unwrap();
        let b_id = b.adopt_remote_game(uuid).unwrap().game.id;
        assert_eq!(icon_sha(&b, b_id), None);
        write_manifest(b_data.path(), &appids);

        // A renames it; B's next sync pulls that before looking the icon up.
        let form = GameForm {
            id: Some(id),
            title: "Ember Trail II".into(),
        };
        a.save_game(form, None).unwrap();
        a.refresh_sync_with(offline, at(0)).unwrap();
        let log = Default::default();
        let items = get_items(&[(900000002, Some(HASH_A))]);
        b.refresh_sync_with(fake_steam(&log, &items, || Ok(jpg(184))), at(0))
            .unwrap();
        let log = log.into_inner();
        assert!(log[0].contains("%7B%22appid%22%3A900000002%7D"), "{log:?}");
        assert!(!log[0].contains("900000001"));
        assert!(icon_sha(&b, b_id).is_some());

        assert_eq!(icon_sha(&a, id), None);
        // A pulls the icon. It is automatic and under 256 px, so A asks Steam
        // once too; the same icon writes nothing.
        let log = Default::default();
        a.refresh_sync_with(fake_steam(&log, &items, || Ok(jpg(184))), at(0))
            .unwrap();
        assert_eq!(icon_sha(&a, id), icon_sha(&b, b_id));
        let meta = |core: &WorldlineCore, id| {
            let slug = core.get_game(id).unwrap().game.folder_slug;
            core.history().store.game_meta(&slug).unwrap().unwrap()
        };
        assert_eq!(meta(&a, id), meta(&b, b_id), "not rewritten on A");
        assert_eq!(meta(&a, id).icon_source, store::IconSource::Auto);
    }

    // ---- Upgrading automatic icons (phase 4) --------------------------------------

    const RED: [u8; 4] = [255, 0, 0, 255];
    const GREEN: [u8; 4] = [0, 255, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];

    /// A solid `color` image file of `size` px in `dir`.
    fn png_file(dir: &Path, size: u32, color: [u8; 4]) -> String {
        let name = color.map(|c| c.to_string()).join("-");
        let path = dir.join(format!("{size}-{name}.png"));
        image::RgbaImage::from_pixel(size, size, image::Rgba(color))
            .save(&path)
            .unwrap();
        path.to_string_lossy().into_owned()
    }

    fn game_meta(core: &WorldlineCore, id: i32) -> store::GameMeta {
        core.history().game_meta(id).unwrap()
    }

    fn stored_size(core: &WorldlineCore, id: i32) -> (u32, u32) {
        let slug = core.get_game(id).unwrap().game.folder_slug;
        let png = std::fs::read(core.history().store.icon_path(&slug)).unwrap();
        crate::icon::dimensions(&png).unwrap()
    }

    /// What a scan found for `title`: an install whose icon is the file at `icon`.
    fn file_install(title: &str, icon: &str) -> (String, ScannedInstall) {
        (
            store::normalize_display_name(title),
            ScannedInstall {
                title: title.into(),
                install_path: None,
                icon_path: Some(icon.into()),
                steam_appid: None,
            },
        )
    }

    #[test]
    fn an_automatic_icon_only_replaces_none_or_a_smaller_automatic_one() {
        let (core, ids, _data, saves) = steam_games(&["Celeste"], &[]);
        let (dir, id) = (saves.path(), ids[0]);
        let auto = store::IconSource::Auto;
        core.set_game_icon_as(id, &png_file(dir, 32, RED), auto)
            .unwrap();
        assert_eq!(game_meta(&core, id).icon_source, auto, "Add Games' icon");
        assert_eq!(stored_size(&core, id), (32, 32));

        let before = game_meta(&core, id);
        for size in [32, 16] {
            core.set_game_icon_as(id, &png_file(dir, size, BLUE), auto)
                .unwrap();
            assert_eq!(game_meta(&core, id), before, "{size} px: nothing written");
        }
        core.set_game_icon_as(id, &png_file(dir, 64, BLUE), auto)
            .unwrap();
        assert_eq!(stored_size(&core, id), (64, 64));
        assert_eq!(stored_pixel(&core, id), BLUE);

        // The user's pick replaces it even when smaller, and nothing
        // automatic replaces the user's.
        core.set_game_icon(id, &png_file(dir, 16, GREEN)).unwrap();
        assert_eq!(game_meta(&core, id).icon_source, store::IconSource::User);
        let picked = game_meta(&core, id);
        let path = core
            .set_game_icon_as(id, &png_file(dir, 512, BLUE), auto)
            .unwrap();
        assert_eq!(game_meta(&core, id), picked);
        assert_eq!(Some(path), core.get_game(id).unwrap().game.icon_path);
    }

    #[test]
    fn a_small_automatic_icon_is_upgraded_by_steam_and_never_downgraded() {
        let (core, ids, data, saves) = steam_games(&["Celeste"], &[("Celeste", 504230)]);
        let id = ids[0];
        core.set_game_icon_as(
            id,
            &png_file(saves.path(), 32, RED),
            store::IconSource::Auto,
        )
        .unwrap();
        let items = get_items(&[(504230, Some(HASH_A))]);
        let log = Default::default();
        let steam = fake_steam(&log, &items, || Ok(jpg(184)));
        assert_eq!(core.fill_steam_icons(steam, at(0)).unwrap(), 1);
        assert_eq!(stored_size(&core, id), (184, 184));
        assert_eq!(game_meta(&core, id).icon_source, store::IconSource::Auto);
        let upgraded = game_meta(&core, id);

        // Looked up already: not asked again within the week.
        let asked = |url: &str| -> Result<Vec<u8>> { panic!("asked again: {url}") };
        assert_eq!(core.fill_steam_icons(asked, at(6)).unwrap(), 0);
        // Then the same size, or only the 32 px one, writes nothing.
        let same = fake_steam(&log, &items, || Ok(jpg(184)));
        assert_eq!(core.fill_steam_icons(same, at(7)).unwrap(), 0);
        assert_eq!(game_meta(&core, id), upgraded, "the same size");
        let smaller = fake_steam(&log, &items, || Err(http_status(404)));
        assert_eq!(core.fill_steam_icons(smaller, at(14)).unwrap(), 0);
        assert_eq!(game_meta(&core, id), upgraded, "the 32 px one");
        assert_eq!(lookups(data.path())[&uuid_of(&core, id)], at(14));
    }

    #[test]
    fn full_size_users_and_legacy_icons_are_never_looked_up_or_replaced() {
        let titles = ["Celeste", "Doom", "Legacy"];
        let (core, ids, _data, saves) =
            steam_games(&titles, &[("Celeste", 1), ("Doom", 2), ("Legacy", 3)]);
        let dir = saves.path();
        let auto = store::IconSource::Auto;
        core.set_game_icon_as(ids[0], &png_file(dir, 256, RED), auto)
            .unwrap();
        core.set_game_icon(ids[1], &png_file(dir, 32, RED)).unwrap();
        // Stored before icons had a source: counted as the user's.
        core.set_game_icon_as(ids[2], &png_file(dir, 32, BLUE), auto)
            .unwrap();
        let slug = core.get_game(ids[2]).unwrap().game.folder_slug;
        let path = core.history().store.game_dir(&slug).join("metadata.json");
        let mut legacy: serde_json::Value = read_json(&path).unwrap();
        legacy.as_object_mut().unwrap().remove("icon_source");
        write_json(&path, &legacy).unwrap();
        assert_eq!(
            game_meta(&core, ids[2]).icon_source,
            store::IconSource::User
        );

        let before: Vec<_> = ids.iter().map(|id| game_meta(&core, *id)).collect();
        let steam = |url: &str| -> Result<Vec<u8>> { panic!("asked Steam: {url}") };
        assert_eq!(core.fill_steam_icons(steam, at(0)).unwrap(), 0);
        assert!(core.games_wanting_icons(at(0)).unwrap().is_empty());
        core.installs_for_icons(|| panic!("scanned"), at(0))
            .unwrap();
        let sharper = png_file(dir, 512, GREEN);
        let installs: HashMap<_, _> = titles
            .iter()
            .map(|title| file_install(title, &sharper))
            .collect();
        assert_eq!(core.fill_icons(&installs, steam, Utc::now()).unwrap(), 0);
        let after: Vec<_> = ids.iter().map(|id| game_meta(&core, *id)).collect();
        assert_eq!(after, before);
    }

    #[test]
    fn an_install_upgrade_follows_the_accent_rules_and_rereads_the_install_weekly() {
        let (core, id, _data, _saves) = changed_game();
        let files = tempfile::tempdir().unwrap();
        core.set_game_icon_as(
            id,
            &png_file(files.path(), 32, RED),
            store::IconSource::Auto,
        )
        .unwrap();
        assert_eq!(game_meta(&core, id).accent_hue, Some(0));
        let source = files.path().join("install.png");
        image::RgbaImage::from_pixel(64, 64, image::Rgba(BLUE))
            .save(&source)
            .unwrap();
        let installs = HashMap::from([file_install("Hollow Knight", &source.to_string_lossy())]);
        let steam = |url: &str| -> Result<Vec<u8>> { panic!("asked Steam: {url}") };
        let now = Utc::now();
        assert_eq!(core.fill_icons(&installs, steam, now).unwrap(), 1);
        assert_eq!(stored_size(&core, id), (64, 64));
        assert_eq!(
            game_meta(&core, id).accent_hue,
            Some(240),
            "the automatic hue follows"
        );

        // The user picks a hue; then the install gets a sharper icon. Its
        // cached preview stands in for it until it is a week old.
        core.set_game_accent(id, Some(100)).unwrap();
        image::RgbaImage::from_pixel(128, 128, image::Rgba(GREEN))
            .save(&source)
            .unwrap();
        assert_eq!(core.fill_icons(&installs, steam, now).unwrap(), 0);
        assert_eq!(stored_size(&core, id), (64, 64));
        let later = now + chrono::Duration::days(8);
        assert_eq!(core.fill_icons(&installs, steam, later).unwrap(), 1);
        assert_eq!(stored_size(&core, id), (128, 128));
        assert_eq!(stored_pixel(&core, id), GREEN);
        let meta = game_meta(&core, id);
        assert_eq!(
            (meta.accent_hue, meta.accent_hue_source, meta.icon_source),
            (
                Some(100),
                store::AccentHueSource::User,
                store::IconSource::Auto
            )
        );
    }

    /// Like a fill, an upgrade downloads outside the repository lock and
    /// stores nothing if the game changed meanwhile.
    #[test]
    fn an_upgrade_never_overwrites_what_changed_while_the_icon_downloaded() {
        type Change = Box<dyn Fn(&WorldlineCore, i32, &Path)>;
        let changes: [(&str, Change); 4] = [
            (
                "a user's icon",
                Box::new(|core, id, dir| {
                    core.set_game_icon(id, &png_file(dir, 48, GREEN)).unwrap();
                }),
            ),
            (
                "a sharper icon",
                Box::new(|core, id, dir| {
                    let png = png_file(dir, 256, GREEN);
                    core.set_game_icon_as(id, &png, store::IconSource::Auto)
                        .unwrap();
                }),
            ),
            (
                "a rename",
                Box::new(|core, id, _| {
                    let form = GameForm {
                        id: Some(id),
                        title: "Celeste Classic".into(),
                    };
                    core.save_game(form, None).unwrap();
                }),
            ),
            (
                "a deletion",
                Box::new(|core, id, _| core.delete_game(id).unwrap()),
            ),
        ];
        let items = get_items(&[(504230, Some(HASH_A))]);
        for (change, apply) in changes {
            let (core, ids, _data, saves) = steam_games(&["Celeste"], &[("Celeste", 504230)]);
            let id = ids[0];
            let auto = store::IconSource::Auto;
            core.set_game_icon_as(id, &png_file(saves.path(), 32, RED), auto)
                .unwrap();
            let uuid = uuid_of(&core, id);
            let small = game_meta(&core, id).icon_sha256;
            let log = Default::default();
            let steam = fake_steam(&log, &items, || {
                apply(&core, id, saves.path());
                Ok(jpg(184))
            });
            assert_eq!(core.fill_steam_icons(steam, at(0)).unwrap(), 0, "{change}");
            let history = core.history();
            let meta = history
                .store
                .game_metas()
                .unwrap()
                .into_iter()
                .find(|meta| meta.uuid == uuid)
                .unwrap();
            let size = || {
                let png = std::fs::read(history.store.icon_path(&meta.folder_slug)).unwrap();
                crate::icon::dimensions(&png).unwrap()
            };
            match change {
                "a user's icon" => {
                    assert_eq!(
                        (meta.icon_source, size()),
                        (store::IconSource::User, (48, 48))
                    );
                }
                "a sharper icon" => assert_eq!((meta.icon_source, size()), (auto, (256, 256))),
                _ => assert_eq!(meta.icon_sha256, small, "{change}"),
            }
            if change == "a deletion" {
                assert!(meta.deleted_at.is_some(), "not brought back");
            }
        }
    }
}
