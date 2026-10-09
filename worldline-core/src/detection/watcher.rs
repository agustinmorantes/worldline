use super::RunningGames;
use super::scheduler::Scheduler;
use super::shots::{Attach, Change, Shots};
use super::{CaptureTarget, Notification, Suppression};
use crate::history::CheckpointOutcome;
use crate::models::{FullGame, MonitoringSettings};
use crate::screenshot::{self, Worker};
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
    /// A checkpoint completed: `created` is the new version's id.
    Checkpointed {
        game_id: i32,
        created: Option<i32>,
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
    pub fn completed(&self, game_id: i32, outcome: &CheckpointOutcome) {
        let created = match outcome {
            CheckpointOutcome::Created(version) => Some(version.id),
            CheckpointOutcome::Unchanged(_) | CheckpointOutcome::Empty => None,
        };
        let _ = self.0.send(Event::Checkpointed { game_id, created });
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
                shots: Shots::default(),
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
    /// The screenshot thread, while screenshots are on.
    worker: Option<Worker>,
}

impl Loop {
    fn run(mut self, rx: mpsc::Receiver<Event>) {
        let mut debouncer = self.debouncer();
        self.reload(&mut debouncer, false);
        self.set_worker(self.settings.screenshots_enabled);
        // One checkpoint per game on startup catches edits made while closed;
        // it is a no-op when nothing changed.
        for game in &self.games {
            self.scheduler.changed(game.game.id);
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
                    self.settings = settings;
                    if rebuild {
                        debouncer = self.debouncer();
                        self.reload(&mut debouncer, false);
                    }
                    self.set_worker(settings.screenshots_enabled);
                }
                Ok(Event::Checkpointed { game_id, created }) => {
                    let attach = self.shots.checkpointed(game_id, created);
                    self.attach(game_id, attach);
                }
                Ok(Event::Shot { game_id, jpeg }) => {
                    let attach = self.shots.finished(game_id, jpeg);
                    self.attach(game_id, attach);
                }
                Ok(Event::Shutdown) | Err(RecvTimeoutError::Disconnected) => {
                    // Events still buffered in the debouncer are lost with it, so every
                    // game is checked once more; unchanged games are a no-op.
                    for game in &self.games {
                        self.scheduler.changed(game.game.id);
                    }
                    for game in self.scheduler.drain() {
                        self.capture(game);
                    }
                    // The shot in progress finishes; then each shot meets the
                    // checkpoint the flush just completed.
                    self.set_worker(false);
                    for event in rx.try_iter() {
                        match event {
                            Event::Checkpointed { game_id, created } => {
                                let attach = self.shots.checkpointed(game_id, created);
                                self.attach(game_id, attach);
                            }
                            Event::Shot { game_id, jpeg } => {
                                let attach = self.shots.finished(game_id, jpeg);
                                self.attach(game_id, attach);
                            }
                            _ => {}
                        }
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
        }
    }

    /// Starts the screenshot worker while screenshots are on (and supported),
    /// and stops it when they're turned off.
    fn set_worker(&mut self, on: bool) {
        let on = on && screenshot::supported();
        if on == self.worker.is_some() {
            return;
        }
        self.worker = on.then(|| {
            let tx = self.tx.clone();
            Worker::start(Box::new(move |game_id, jpeg| {
                let _ = tx.send(Event::Shot { game_id, jpeg });
            }))
        });
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

    fn attach(&self, game_id: i32, attach: Option<Attach>) {
        let Some(Attach { version_id, jpeg }) = attach else {
            return;
        };
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
            Ok(CheckpointOutcome::Created(version)) => {
                self.notify(Notification::VersionCreated {
                    game_id,
                    version_number: version.version_number,
                });
                if let Ok(games) = self.target.games() {
                    self.games = games;
                }
                true
            }
            Ok(CheckpointOutcome::Unchanged(_) | CheckpointOutcome::Empty) => true,
            Err(error) => {
                self.notify(Notification::Error(format!(
                    "Automatic checkpoint failed: {error:#}"
                )));
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
                game: Game {
                    id: 1,
                    uuid: String::new(),
                    title: "G".into(),
                    title_normalized: "g".into(),
                    folder_slug: "g__g_1".into(),
                    active_slot_id: None,
                    icon_path: None,
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
            }])
        }

        fn capture(&self, _game_id: i32) -> anyhow::Result<CheckpointOutcome> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(CheckpointOutcome::Empty)
        }

        fn attach_screenshot(&self, _version_id: i32, _jpeg: &[u8]) -> anyhow::Result<()> {
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
}
