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
        let Plan { sync, push, stop } = coalesce(
            std::iter::once(next).chain(triggers.try_iter()),
            due,
            Instant::now(),
        );
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
    match core.push_sync() {
        // Refresh the shown report; this device's pointers just moved.
        Ok(()) => {
            if let Ok(report) = core.sync_status() {
                let _ = notifications.send(Notification::SyncCompleted(report));
            }
        }
        Err(error) => {
            let _ = notifications.send(Notification::SyncFailed(format!("{error:#}")));
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Plan {
    sync: bool,
    push: bool,
    stop: bool,
}

/// Merges queued triggers. A periodic sync that came due counts even when a
/// push arrived first, so steady pushes cannot starve pull and pruning.
fn coalesce(triggers: impl Iterator<Item = Trigger>, due: Instant, now: Instant) -> Plan {
    let mut plan = Plan {
        sync: now >= due,
        push: false,
        stop: false,
    };
    for trigger in triggers {
        match trigger {
            Trigger::Sync => plan.sync = true,
            Trigger::Push => plan.push = true,
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
        let queued = || [Trigger::Push, Trigger::Push].into_iter();
        let plan = coalesce(queued(), now - Duration::from_secs(1), now);
        assert!(plan.sync && plan.push);
        let plan = coalesce(queued(), now + TICK, now);
        assert!(!plan.sync && plan.push, "not due yet: push only");
    }
}
