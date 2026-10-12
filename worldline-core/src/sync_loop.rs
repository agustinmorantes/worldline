//! Background sync: one thread that turns "a version was created", "five
//! minutes passed" and "shutting down" into pushes and full syncs, so no
//! front end schedules sync itself.

use crate::core_facade::WorldlineCore;
use crate::detection::Notification;
use crate::error::Failure;
use std::collections::HashSet;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use uuid::Uuid;

const TICK: Duration = Duration::from_secs(5 * 60);

pub(crate) enum Trigger {
    Sync,
    /// A game got a new version.
    Push(Uuid),
    Stop,
}

pub(crate) struct SyncLoop {
    trigger: mpsc::Sender<Trigger>,
    thread: JoinHandle<()>,
}

impl SyncLoop {
    /// Runs a full sync at once, then on every trigger or tick.
    pub(crate) fn start(
        core: Weak<WorldlineCore>,
        triggers: mpsc::Receiver<Trigger>,
        trigger: mpsc::Sender<Trigger>,
        notifications: mpsc::Sender<Notification>,
    ) -> Self {
        let thread = std::thread::spawn(move || run(core, triggers, notifications));
        Self { trigger, thread }
    }

    /// Drains the queue, pushes once and waits for the thread. When the loop
    /// itself dropped the last handle to the core, there is nobody to wait for.
    pub(crate) fn stop(self) {
        let _ = self.trigger.send(Trigger::Stop);
        if self.thread.thread().id() != std::thread::current().id() {
            let _ = self.thread.join();
        }
    }
}

fn run(
    core: Weak<WorldlineCore>,
    triggers: mpsc::Receiver<Trigger>,
    notifications: mpsc::Sender<Notification>,
) {
    let mut next = Trigger::Sync;
    let mut due = Instant::now();
    loop {
        // Coalesce whatever queued up while working.
        let Plan { sync, push, stop } = coalesce(
            std::iter::once(next).chain(triggers.try_iter()),
            due,
            Instant::now(),
        );
        let Some(core) = core.upgrade() else { return };
        // Shutting down only publishes; a pull or prune could keep the exit waiting.
        // It publishes every game, like a full sync, catching whatever a game's
        // own push left out.
        if stop {
            push_only(&core, &notifications, None);
        } else if sync {
            full_sync(&core, &notifications);
            due = Instant::now() + TICK;
        } else if !push.is_empty() {
            push_only(&core, &notifications, Some(&push));
        }
        if stop {
            return;
        }
        drop(core);
        next = match triggers.recv_timeout(due.saturating_duration_since(Instant::now())) {
            Ok(trigger) => trigger,
            Err(RecvTimeoutError::Timeout) => Trigger::Sync,
            Err(RecvTimeoutError::Disconnected) => return,
        };
    }
}

/// Unreadable settings count as configured, so the failure gets reported.
fn sync_configured(core: &WorldlineCore) -> bool {
    core.get_sync_settings()
        .map_or(true, |settings| settings.remote_path.is_some())
}

/// Every sync and push that runs is announced, then reported.
fn full_sync(core: &Arc<WorldlineCore>, notifications: &mpsc::Sender<Notification>) {
    if sync_configured(core) {
        let _ = notifications.send(Notification::SyncStarted);
        match core.refresh_sync() {
            Ok(report) => {
                let _ = notifications.send(Notification::SyncCompleted(report));
            }
            Err(error) => {
                let _ = notifications.send(Notification::SyncFailed(Failure::from(&error)));
            }
        }
    }
    match core.prune_if_due() {
        Ok(Some(report)) if report.changed_history() => {
            let _ = notifications.send(Notification::HistoryPruned);
        }
        Ok(_) => {}
        Err(error) => {
            let _ = notifications.send(Notification::PruningFailed(Failure::from(&error)));
        }
    }
}

/// Pushes `games`, or every game.
fn push_only(
    core: &Arc<WorldlineCore>,
    notifications: &mpsc::Sender<Notification>,
    games: Option<&HashSet<Uuid>>,
) {
    if !sync_configured(core) {
        return;
    }
    let _ = notifications.send(Notification::SyncStarted);
    // Refresh the shown report; this device's pointers just moved.
    let pushed = match games {
        Some(games) => core
            .push_games(games)
            .and_then(|()| core.pushed_status(games)),
        None => core.push_sync().and_then(|()| core.sync_status()),
    };
    match pushed {
        Ok(report) => {
            let _ = notifications.send(Notification::SyncCompleted(report));
        }
        Err(error) => {
            let _ = notifications.send(Notification::SyncFailed(Failure::from(&error)));
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Plan {
    sync: bool,
    /// Games to push.
    push: HashSet<Uuid>,
    stop: bool,
}

/// Merges queued triggers. A periodic sync that came due counts even when a
/// push arrived first, so steady pushes cannot starve pull and pruning.
fn coalesce(triggers: impl Iterator<Item = Trigger>, due: Instant, now: Instant) -> Plan {
    let mut plan = Plan {
        sync: now >= due,
        push: HashSet::new(),
        stop: false,
    };
    for trigger in triggers {
        match trigger {
            Trigger::Sync => plan.sync = true,
            Trigger::Push(game) => {
                plan.push.insert(game);
            }
            Trigger::Stop => plan.stop = true,
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_due_sync_is_not_starved_by_pushes() {
        let now = Instant::now();
        let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let queued = || [Trigger::Push(a), Trigger::Push(b), Trigger::Push(a)].into_iter();
        let plan = coalesce(queued(), now - Duration::from_secs(1), now);
        assert!(plan.sync && !plan.push.is_empty());
        let plan = coalesce(queued(), now + TICK, now);
        assert!(!plan.sync, "not due yet: push only");
        assert_eq!(plan.push, HashSet::from([a, b]), "each game pushed once");
    }
}
