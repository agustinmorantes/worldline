//! Background sync: one thread that turns "a version was created", "five
//! minutes passed" and "shutting down" into pushes and full syncs, so no
//! front end schedules sync itself.

use crate::core_facade::TimelineCore;
use crate::detection::Notification;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const TICK: Duration = Duration::from_secs(5 * 60);

pub(crate) enum Trigger {
    Sync,
    Push,
    Stop,
}

pub(crate) struct SyncLoop {
    trigger: mpsc::Sender<Trigger>,
    thread: JoinHandle<()>,
}

impl SyncLoop {
    /// Runs a full sync at once, then on every trigger or tick.
    pub(crate) fn start(
        core: Weak<TimelineCore>,
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
    core: Weak<TimelineCore>,
    triggers: mpsc::Receiver<Trigger>,
    notifications: mpsc::Sender<Notification>,
) {
    let mut next = Trigger::Sync;
    let mut due = Instant::now();
    loop {
        // Coalesce whatever queued up while working.
        let (mut sync, mut push, mut stop) = (false, false, false);
        for trigger in std::iter::once(next).chain(triggers.try_iter()) {
            match trigger {
                Trigger::Sync => sync = true,
                Trigger::Push => push = true,
                Trigger::Stop => stop = true,
            }
        }
        let Some(core) = core.upgrade() else { return };
        // Shutting down only publishes; a pull or prune could keep the exit waiting.
        if stop || (push && !sync) {
            push_only(&core, &notifications);
        } else if sync {
            full_sync(&core, &notifications);
            due = Instant::now() + TICK;
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

fn full_sync(core: &Arc<TimelineCore>, notifications: &mpsc::Sender<Notification>) {
    match core.sync_if_configured() {
        Ok(Some(report)) => {
            let _ = notifications.send(Notification::SyncCompleted(report));
        }
        Ok(None) => {}
        Err(error) => {
            let _ = notifications.send(Notification::SyncFailed(format!("{error:#}")));
        }
    }
    match core.prune_if_due() {
        Ok(Some(report)) if report.changed_history() => {
            let _ = notifications.send(Notification::HistoryPruned);
        }
        Ok(_) => {}
        Err(error) => {
            let _ = notifications.send(Notification::PruningFailed(format!("{error:#}")));
        }
    }
}

fn push_only(core: &Arc<TimelineCore>, notifications: &mpsc::Sender<Notification>) {
    if let Err(error) = core.push_sync() {
        let _ = notifications.send(Notification::SyncFailed(format!("{error:#}")));
    }
}
