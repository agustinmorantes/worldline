//! Automatic capture. The notify watcher only reports "game changed"; the
//! scheduler decides when to capture; captures go through `CaptureTarget`.

pub mod scheduler;
mod watcher;

use crate::history::CheckpointOutcome;
use crate::models::FullGame;
use anyhow::Result;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

pub use watcher::Watcher;

/// What the watcher needs from the core.
pub trait CaptureTarget: Send + Sync + 'static {
    /// The games to watch.
    fn games(&self) -> Result<Vec<FullGame>>;
    fn capture(&self, game_id: i32) -> Result<CheckpointOutcome>;
}

pub type NotificationReceiver = mpsc::Receiver<Notification>;

#[derive(Debug, Clone, PartialEq)]
pub enum Notification {
    ChangeDetected { game_id: i32 },
    VersionCreated { game_id: i32, version_number: i32 },
    Error(String),
}

/// A nesting suppression counter: while any guard is alive, nothing is
/// captured automatically. Pending changes are kept and handled on resume.
#[derive(Clone, Default)]
pub struct Suppression(Arc<AtomicUsize>);

impl Suppression {
    pub fn is_active(&self) -> bool {
        self.0.load(Ordering::SeqCst) > 0
    }

    pub fn hold(&self) -> SuppressionGuard {
        self.0.fetch_add(1, Ordering::SeqCst);
        SuppressionGuard(self.clone())
    }
}

pub struct SuppressionGuard(Suppression);

impl Drop for SuppressionGuard {
    fn drop(&mut self) {
        self.0.0.fetch_sub(1, Ordering::SeqCst);
    }
}
