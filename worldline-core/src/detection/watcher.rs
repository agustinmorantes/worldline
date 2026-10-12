use super::RunningGames;
use super::scheduler::Scheduler;
use super::shots::{self, Attach, Change, Shots};
use super::worker::{Shoot, Worker};
use super::{CaptureFailure, CaptureTarget, Notification, Suppression};
use crate::history::CheckpointOutcome;
use crate::models::{FullGame, MonitoringSettings};
use crate::screenshot;
use chrono::{Duration, NaiveDateTime, Utc};
use notify_debouncer_full::notify::{RecommendedWatcher, RecursiveMode, Watcher as _};
use notify_debouncer_full::{DebounceEventResult, Debouncer, FileIdMap, new_debouncer};
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::JoinHandle;

/// How often a suppressed watcher re-checks whether it may capture again.
const SUPPRESSED_POLL: std::time::Duration = std::time::Duration::from_millis(250);

enum Event {
    Fs(DebounceEventResult),
    Reload,
    Settings(MonitoringSettings),
    /// A checkpoint that `started` before reading the saves completed:
    /// `created` is the new version's id.
    Checkpointed {
        game_id: i32,
        created: Option<i32>,
        started: NaiveDateTime,
    },
    /// The screenshot worker finished a shot; `None` when it took none.
    Shot {
        game_id: i32,
        jpeg: Option<Vec<u8>>,
    },
    Shutdown,
}

/// Tells the watcher about every completed checkpoint, however it was
/// taken, so a screenshot finds its version.
#[derive(Clone)]
pub struct Checkpoints(mpsc::Sender<Event>);

impl Checkpoints {
    /// `started`: before the checkpoint read the saves.
    pub fn completed(&self, game_id: i32, started: NaiveDateTime, outcome: &CheckpointOutcome) {
        let created = match outcome {
            CheckpointOutcome::Created(version) => Some(version.id),
            CheckpointOutcome::Unchanged(_) | CheckpointOutcome::Empty => None,
        };
        let _ = self.0.send(Event::Checkpointed {
            game_id,
            created,
            started,
        });
    }
}

/// Handle to the watcher thread. Dropping it flushes pending captures.
pub struct Watcher {
    tx: mpsc::Sender<Event>,
    thread: Option<JoinHandle<()>>,
}

impl Watcher {
    /// `running` is read to screenshot running games.
    pub fn start(
        target: Arc<dyn CaptureTarget>,
        settings: MonitoringSettings,
        suppression: Suppression,
        running: RunningGames,
        notifications: mpsc::Sender<Notification>,
    ) -> Self {
        let shooter = screenshot::supported().then_some(screenshot::shooter as fn() -> Shoot);
        Self::start_with(
            target,
            settings,
            suppression,
            running,
            notifications,
            shooter,
            shots::FLOOR,
        )
    }

    /// `shooter`: how the screenshot worker takes shots; `None` when it can't.
    /// `floor`: at least this long between two shots of a game.
    fn start_with(
        target: Arc<dyn CaptureTarget>,
        settings: MonitoringSettings,
        suppression: Suppression,
        running: RunningGames,
        notifications: mpsc::Sender<Notification>,
        shooter: Option<fn() -> Shoot>,
        floor: Duration,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let loop_tx = tx.clone();
        let thread = std::thread::spawn(move || {
            Loop {
                target,
                settings,
                suppression,
                notifications,
                tx: loop_tx,
                games: Vec::new(),
                scheduler: Scheduler::default(),
                running,
                shots: Shots::with_floor(floor),
                shooter,
                worker: None,
            }
            .run(rx)
        });
        Self {
            tx,
            thread: Some(thread),
        }
    }

    /// Re-reads the game list (after games or tracked directories change).
    pub fn reload(&self) {
        if self.tx.send(Event::Reload).is_err() {
            eprintln!("Watcher is not running; tracked directories were not re-registered");
        }
    }

    pub fn checkpoints(&self) -> Checkpoints {
        Checkpoints(self.tx.clone())
    }

    pub fn update_settings(&self, settings: MonitoringSettings) {
        if self.tx.send(Event::Settings(settings)).is_err() {
            eprintln!("Watcher is not running; monitoring settings apply on restart");
        }
    }

    /// Captures every pending change and stops the thread.
    pub fn shutdown(&mut self) {
        let _ = self.tx.send(Event::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct Loop {
    target: Arc<dyn CaptureTarget>,
    settings: MonitoringSettings,
    suppression: Suppression,
    notifications: mpsc::Sender<Notification>,
    tx: mpsc::Sender<Event>,
    games: Vec<FullGame>,
    scheduler: Scheduler,
    running: RunningGames,
    shots: Shots,
    shooter: Option<fn() -> Shoot>,
    /// The screenshot thread, while screenshots are on.
    worker: Option<Worker>,
}

impl Loop {
    fn run(mut self, rx: mpsc::Receiver<Event>) {
        let mut debouncer = self.debouncer();
        self.reload(&mut debouncer, false);
        self.set_worker(self.settings.screenshots_enabled);
        // One checkpoint per game on startup catches edits made while closed;
        // it is a no-op when nothing changed. One the cooldown defers still
        // shows them as unrecorded.
        for game in &self.games {
            self.scheduler.changed(game.game.id);
            self.check_unrecorded(game.game.id);
        }
        loop {
            let now = Utc::now().naive_utc();
            let timeout = if self.suppression.is_active() {
                self.scheduler
                    .next_wakeup(now, |id| self.eligible_at(id))
                    .map(|_| SUPPRESSED_POLL)
            } else {
                self.scheduler.next_wakeup(now, |id| self.eligible_at(id))
            };
            let event = match timeout {
                Some(timeout) => rx.recv_timeout(timeout),
                None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            match event {
                Ok(Event::Fs(Ok(events))) => {
                    for event in events {
                        self.mark_changed(&event.paths, event.need_rescan());
                    }
                }
                Ok(Event::Fs(Err(errors))) => {
                    self.notify(Notification::Error(format!("Watcher error: {errors:?}")))
                }
                Ok(Event::Reload) => self.reload(&mut debouncer, true),
                Ok(Event::Settings(settings)) => {
                    let rebuild = (settings.snapshot_debounce_seconds
                        - self.settings.snapshot_debounce_seconds)
                        .abs()
                        > f32::EPSILON;
                    let cooldown =
                        settings.version_cooldown_minutes != self.settings.version_cooldown_minutes;
                    self.settings = settings;
                    if rebuild {
                        debouncer = self.debouncer();
                        self.reload(&mut debouncer, false);
                    }
                    self.set_worker(settings.screenshots_enabled);
                    // A new cooldown moves when pending games are due.
                    if cooldown {
                        for game_id in self.scheduler.pending() {
                            self.check_unrecorded(game_id);
                        }
                    }
                }
                Ok(event @ (Event::Checkpointed { .. } | Event::Shot { .. })) => self.settle(event),
                Ok(Event::Shutdown) | Err(RecvTimeoutError::Disconnected) => {
                    // Events still buffered in the debouncer are lost with it, so every
                    // game is checked once more; unchanged games are a no-op.
                    for game in &self.games {
                        self.scheduler.changed(game.game.id);
                    }
                    for game in self.scheduler.drain() {
                        self.capture(game);
                    }
                    // The shot in progress finishes (the join); then each shot
                    // meets the checkpoint the flush just completed.
                    drop(self.worker.take());
                    for event in rx.try_iter() {
                        self.settle(event);
                    }
                    return;
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
            if !self.suppression.is_active() {
                let now = Utc::now().naive_utc();
                let due = {
                    let games = &self.games;
                    let cooldown = self.settings.version_cooldown_minutes;
                    self.scheduler
                        .due(now, |id| eligible_at(games, cooldown, id))
                };
                for game in due {
                    if self.capture(game) {
                        self.scheduler.completed(game);
                    } else {
                        self.scheduler.failed(game, Utc::now().naive_utc());
                        self.check_unrecorded(game);
                    }
                }
            }
        }
    }

    fn eligible_at(&self, id: i32) -> Option<Option<NaiveDateTime>> {
        eligible_at(&self.games, self.settings.version_cooldown_minutes, id)
    }

    fn debouncer(&self) -> Option<Debouncer<RecommendedWatcher, FileIdMap>> {
        let tx = self.tx.clone();
        let delay =
            std::time::Duration::from_secs_f32(self.settings.snapshot_debounce_seconds.max(0.01));
        match new_debouncer(delay, None, move |result| {
            let _ = tx.send(Event::Fs(result));
        }) {
            Ok(debouncer) => Some(debouncer),
            Err(error) => {
                self.notify(Notification::Error(format!(
                    "Failed to start the file watcher: {error}"
                )));
                None
            }
        }
    }

    fn reload(
        &mut self,
        debouncer: &mut Option<Debouncer<RecommendedWatcher, FileIdMap>>,
        unwatch: bool,
    ) {
        if unwatch && let Some(debouncer) = debouncer.as_mut() {
            for dir in self.games.iter().flat_map(|game| &game.tracked_dirs) {
                let _ = debouncer.watcher().unwatch(Path::new(&dir.path));
            }
        }
        match self.target.games() {
            Ok(games) => self.games = games,
            Err(error) => self.notify(Notification::Error(format!(
                "Failed to load games: {error:#}"
            ))),
        }
        for id in self.shots.games() {
            if !self.games.iter().any(|game| game.game.id == id) {
                self.shots.forget(id);
            }
        }
        if let Some(debouncer) = debouncer.as_mut() {
            for dir in self.games.iter().flat_map(|game| &game.tracked_dirs) {
                if let Err(error) = debouncer
                    .watcher()
                    .watch(Path::new(&dir.path), RecursiveMode::Recursive)
                {
                    eprintln!("Cannot watch '{}': {error}", dir.path);
                }
            }
        }
    }

    fn mark_changed(&mut self, paths: &[std::path::PathBuf], rescan: bool) {
        let mut changed = Vec::new();
        for game in &self.games {
            let hit = rescan
                || game.tracked_dirs.iter().any(|dir| {
                    let exclusions = crate::exclusions::parse(&dir.exclusions);
                    paths.iter().any(|path| {
                        let Ok(relative) = path.strip_prefix(&dir.path) else {
                            return false;
                        };
                        let excluded_name = path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .is_some_and(crate::fs_utils::is_excluded_file);
                        !excluded_name && !crate::exclusions::is_excluded(&exclusions, relative)
                    })
                });
            if hit {
                changed.push(game.game.id);
            }
        }
        for game_id in changed {
            self.scheduler.changed(game_id);
            self.notify(Notification::ChangeDetected { game_id });
            self.request_shot(game_id, !rescan);
            self.check_unrecorded(game_id);
        }
    }

    /// Records whether the game's live saves differ from its active version,
    /// unless its capture runs now and settles that itself. They're due at the
    /// latest of the cooldown's end, a failed capture's retry and the debounce's end.
    fn check_unrecorded(&self, game_id: i32) {
        let now = Utc::now().naive_utc();
        let Some(eligible) = self.eligible_at(game_id) else {
            return;
        };
        let retry = self.scheduler.retry_at(game_id);
        if !self.suppression.is_active() && retry.is_none() && eligible.is_none_or(|at| at <= now) {
            return;
        }
        let due_at = eligible
            .unwrap_or(now)
            .max(retry.unwrap_or(now))
            .max(now + self.debounce());
        // A check that fails keeps what the last one found; the capture reports real trouble.
        if let Err(error) = self.target.check_unrecorded(game_id, due_at) {
            eprintln!("Cannot check game {game_id} for unrecorded changes: {error:#}");
        }
    }

    fn debounce(&self) -> Duration {
        Duration::milliseconds((self.settings.snapshot_debounce_seconds * 1000.0) as i64)
    }

    /// Starts the screenshot worker while screenshots are on (and supported),
    /// and stops it when they're turned off, without waiting for a shot in
    /// progress: it still arrives as `Event::Shot`.
    fn set_worker(&mut self, on: bool) {
        let shooter = self.shooter.filter(|_| on);
        if shooter.is_some() == self.worker.is_some() {
            return;
        }
        if let Some(shooter) = shooter {
            let tx = self.tx.clone();
            self.worker = Some(Worker::start(
                shooter,
                Box::new(move |game_id, jpeg| {
                    let _ = tx.send(Event::Shot { game_id, jpeg });
                }),
            ));
        } else if let Some(worker) = self.worker.take() {
            worker.detach();
            self.shots.worker_stopped();
        }
    }

    /// Pairs shots with checkpoint outcomes and attaches the matches.
    fn settle(&mut self, event: Event) {
        let attach = match event {
            Event::Checkpointed {
                game_id,
                created,
                started,
            } => {
                let attach = self
                    .shots
                    .checkpointed(game_id, created, started, self.debounce());
                attach.map(|attach| (game_id, attach))
            }
            Event::Shot { game_id, jpeg } => self
                .shots
                .finished(game_id, jpeg)
                .map(|attach| (game_id, attach)),
            _ => None,
        };
        if let Some((game_id, attach)) = attach {
            self.attach(game_id, attach);
        }
    }

    /// Asks the worker for a shot of the game, if `Shots` allows one.
    fn request_shot(&mut self, game_id: i32, file_event: bool) {
        let Some(worker) = &self.worker else {
            return;
        };
        let pids = self.running.pids(game_id);
        let change = Change {
            running: !pids.is_empty(),
            suppressed: self.suppression.is_active(),
            file_event,
        };
        self.shots
            .changed(game_id, change, Utc::now().naive_utc(), || {
                worker.request(game_id, pids)
            });
    }

    fn attach(&self, game_id: i32, Attach { version_id, jpeg }: Attach) {
        match self.target.attach_screenshot(version_id, &jpeg) {
            Ok(()) => self.notify(Notification::ScreenshotAttached {
                game_id,
                version_id,
            }),
            Err(error) => self.notify(Notification::Error(format!(
                "Failed to save a screenshot: {error:#}"
            ))),
        }
    }

    /// Returns whether the capture succeeded.
    fn capture(&mut self, game_id: i32) -> bool {
        match self.target.capture(game_id) {
            Ok((CheckpointOutcome::Created(version), branch_from)) => {
                self.notify(Notification::VersionCreated {
                    game_id,
                    version_number: version.version_number,
                    branch_from,
                });
                if let Ok(games) = self.target.games() {
                    self.games = games;
                }
                true
            }
            Ok((CheckpointOutcome::Unchanged(_) | CheckpointOutcome::Empty, _)) => true,
            Err(error) => {
                self.notify(Notification::Error(format!(
                    "Automatic checkpoint failed: {error:#}"
                )));
                self.notify(Notification::CheckpointFailed {
                    game_id,
                    reason: CaptureFailure::from(&error),
                });
                false
            }
        }
    }

    /// A lost receiver only means nobody listens; capturing continues.
    fn notify(&self, notification: Notification) {
        let _ = self.notifications.send(notification);
    }
}

/// `None`: the game is gone. `Some(None)`: no cooldown applies.
fn eligible_at(
    games: &[FullGame],
    cooldown_minutes: u32,
    id: i32,
) -> Option<Option<NaiveDateTime>> {
    let game = games.iter().find(|game| game.game.id == id)?;
    Some(
        game.versions
            .iter()
            .map(|version| version.created_at)
            .max()
            .map(|created| created + Duration::minutes(i64::from(cooldown_minutes))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::CheckpointOutcome;
    use crate::models::{Game, TrackedDir};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Counting(AtomicUsize);

    impl CaptureTarget for Counting {
        fn games(&self) -> anyhow::Result<Vec<FullGame>> {
            Ok(vec![FullGame {
                last_played_at: None,
                game: Game {
                    id: 1,
                    uuid: String::new(),
                    title: "G".into(),
                    title_normalized: "g".into(),
                    folder_slug: "g__g_1".into(),
                    active_slot_id: None,
                    icon_path: None,
                    icon_sha256: None,
                    install_dir: None,
                    accent_hue: 0,
                    accent_hue_source: "auto".into(),
                },
                tracked_dirs: Vec::<TrackedDir>::new(),
                slots: vec![],
                slot_summaries: vec![],
                active_slot: None,
                versions: vec![],
                last_version: None,
                linked_slot_ids: vec![],
            }])
        }

        fn capture(
            &self,
            _game_id: i32,
        ) -> anyhow::Result<(CheckpointOutcome, Option<super::super::BranchFrom>)> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok((CheckpointOutcome::Empty, None))
        }

        fn attach_screenshot(&self, _version_id: i32, _jpeg: &[u8]) -> anyhow::Result<()> {
            Ok(())
        }

        fn check_unrecorded(&self, _game_id: i32, _due_at: NaiveDateTime) -> anyhow::Result<()> {
            Ok(())
        }
    }

    /// Events still inside the debouncer never reach the scheduler, so
    /// shutdown must capture every game rather than only the pending ones.
    #[test]
    fn shutdown_captures_every_game_even_without_a_pending_change() {
        let target = Arc::new(Counting(AtomicUsize::new(0)));
        let mut watcher = Watcher::start(
            target.clone(),
            MonitoringSettings::default(),
            Suppression::default(),
            RunningGames::default(),
            mpsc::channel().0,
        );
        // The startup checkpoint has run once the scheduler is drained.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while target.0.load(Ordering::SeqCst) < 1 {
            assert!(std::time::Instant::now() < deadline, "no startup capture");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        watcher.shutdown();
        assert_eq!(target.0.load(Ordering::SeqCst), 2);
    }

    /// Checkpoints like `Inner::checkpoint`: each creates a version and is
    /// reported through `Checkpoints`.
    struct Versioning {
        dir: String,
        checkpoints: std::sync::OnceLock<Checkpoints>,
        created: AtomicUsize,
        /// What it created, so a cooldown applies.
        versions: std::sync::Mutex<Vec<crate::models::Version>>,
        attached: std::sync::Mutex<mpsc::Sender<(i32, Vec<u8>)>>,
    }

    impl CaptureTarget for Versioning {
        fn games(&self) -> anyhow::Result<Vec<FullGame>> {
            let mut games = Counting(AtomicUsize::new(0)).games()?;
            games[0].tracked_dirs.push(TrackedDir {
                id: 1,
                uuid: String::new(),
                game_id: 1,
                folder_slug: "saves".into(),
                path: self.dir.clone(),
                label: None,
                exclusions: "[]".into(),
                retired_at: None,
            });
            games[0].versions = self.versions.lock().unwrap().clone();
            Ok(games)
        }

        fn capture(
            &self,
            game_id: i32,
        ) -> anyhow::Result<(CheckpointOutcome, Option<super::super::BranchFrom>)> {
            let started = Utc::now().naive_utc();
            let n = self.created.fetch_add(1, Ordering::SeqCst) as i32;
            let outcome = CheckpointOutcome::Created(crate::models::Version {
                id: 10 + n,
                uuid: String::new(),
                slot_id: 1,
                parent_id: None,
                version_number: n + 1,
                folder_slug: format!("v{n}"),
                reason: "watcher".into(),
                tag: None,
                retired: false,
                created_at: started,
                origin_device: None,
            });
            if let CheckpointOutcome::Created(version) = &outcome {
                self.versions.lock().unwrap().push(version.clone());
            }
            if let Some(checkpoints) = self.checkpoints.get() {
                checkpoints.completed(game_id, started, &outcome);
            }
            Ok((outcome, None))
        }

        fn attach_screenshot(&self, version_id: i32, jpeg: &[u8]) -> anyhow::Result<()> {
            let _ = self
                .attached
                .lock()
                .unwrap()
                .send((version_id, jpeg.to_vec()));
            Ok(())
        }

        fn check_unrecorded(&self, _game_id: i32, _due_at: NaiveDateTime) -> anyhow::Result<()> {
            Ok(())
        }
    }

    /// A shot of the PIDs: `[42]` for PID 42.
    fn fake_shooter() -> Shoot {
        Box::new(|pids| Some(pids.iter().map(|&pid| pid as u8).collect()))
    }

    #[test]
    fn a_save_change_of_a_running_game_attaches_a_shot_to_its_checkpoint() {
        let saves = tempfile::tempdir().unwrap();
        let (attached_tx, attached) = mpsc::channel();
        let target = Arc::new(Versioning {
            dir: saves.path().to_string_lossy().into_owned(),
            checkpoints: Default::default(),
            created: AtomicUsize::new(0),
            versions: Default::default(),
            attached: std::sync::Mutex::new(attached_tx),
        });
        let running = RunningGames::default();
        running.set(1, vec![42]);
        let (notifications_tx, notifications) = mpsc::channel();
        let mut watcher = Watcher::start_with(
            target.clone(),
            MonitoringSettings {
                version_cooldown_minutes: 0,
                snapshot_debounce_seconds: 0.2,
                screenshots_enabled: true,
                ..MonitoringSettings::default()
            },
            Suppression::default(),
            running,
            notifications_tx,
            Some(fake_shooter),
            shots::FLOOR,
        );
        let _ = target.checkpoints.set(watcher.checkpoints());
        // The startup checkpoint (version 10) predates any shot.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while target.created.load(Ordering::SeqCst) < 1 {
            assert!(std::time::Instant::now() < deadline, "no startup capture");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        std::fs::write(saves.path().join("save.dat"), "1").unwrap();
        let (version_id, jpeg) = attached
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("a shot was attached");
        assert!(version_id > 10, "the checkpoint of the change");
        assert_eq!(jpeg, vec![42]);
        watcher.shutdown();
        assert!(notifications.try_iter().any(|notification| matches!(
            notification,
            Notification::ScreenshotAttached { game_id: 1, version_id: id } if id == version_id
        )));
    }

    static SHOTS: AtomicUsize = AtomicUsize::new(0);

    /// Numbers its shots: `[n]` for the nth.
    fn counting_shooter() -> Shoot {
        Box::new(|_| Some(vec![SHOTS.fetch_add(1, Ordering::SeqCst) as u8 + 1]))
    }

    /// Waits for a shot after the `seen`th, and for the watcher to settle it.
    fn wait_for_shot_after(seen: usize) -> usize {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while SHOTS.load(Ordering::SeqCst) <= seen {
            assert!(std::time::Instant::now() < deadline, "no shot");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
        SHOTS.load(Ordering::SeqCst)
    }

    /// Two saves during the cooldown, then a manual checkpoint: it gets the
    /// shot of the second save, not the first.
    #[test]
    fn a_manual_checkpoint_during_the_cooldown_gets_the_shot_of_the_latest_save() {
        let saves = tempfile::tempdir().unwrap();
        let (attached_tx, attached) = mpsc::channel();
        let target = Arc::new(Versioning {
            dir: saves.path().to_string_lossy().into_owned(),
            checkpoints: Default::default(),
            created: AtomicUsize::new(0),
            versions: Default::default(),
            attached: std::sync::Mutex::new(attached_tx),
        });
        let running = RunningGames::default();
        running.set(1, vec![42]);
        let mut watcher = Watcher::start_with(
            target.clone(),
            MonitoringSettings {
                version_cooldown_minutes: 15,
                snapshot_debounce_seconds: 0.2,
                screenshots_enabled: true,
                ..MonitoringSettings::default()
            },
            Suppression::default(),
            running,
            mpsc::channel().0,
            Some(counting_shooter),
            Duration::zero(),
        );
        let _ = target.checkpoints.set(watcher.checkpoints());
        // The startup checkpoint (version 10) starts the cooldown.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while target.created.load(Ordering::SeqCst) < 1 {
            assert!(std::time::Instant::now() < deadline, "no startup capture");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        std::fs::write(saves.path().join("save.dat"), "bone bottom").unwrap();
        let first = wait_for_shot_after(0);
        std::fs::write(saves.path().join("save.dat"), "before miniboss").unwrap();
        let latest = wait_for_shot_after(first);
        assert_eq!(
            target.created.load(Ordering::SeqCst),
            1,
            "still cooling down"
        );
        // Reported the way `WorldlineCore::checkpoint` reports a manual one.
        let (CheckpointOutcome::Created(version), _) = target.capture(1).unwrap() else {
            unreachable!("Versioning always creates");
        };
        let (version_id, jpeg) = attached
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("a shot was attached");
        assert_eq!(version_id, version.id);
        assert_eq!(jpeg, vec![latest as u8], "the shot of the latest save");
        // The shutdown flush's checkpoint gets no shot of an earlier save.
        watcher.shutdown();
        assert!(attached.try_recv().is_err());
    }
}
