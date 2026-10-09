//! Automatic capture. The notify watcher only reports "game changed"; the
//! scheduler decides when to capture; captures go through `CaptureTarget`.
//! `running` reports which games are running; `shots` decides when one is
//! screenshotted.

pub(crate) mod running;
pub mod scheduler;
pub mod shots;
mod watcher;
pub(crate) mod worker;

use crate::error::Failure;
use crate::history::CheckpointOutcome;
use crate::models::FullGame;
use crate::sync::SyncReport;
use anyhow::Result;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

pub use running::RunningGames;
pub use watcher::{Checkpoints, Watcher};

/// What the watcher needs from the core.
pub trait CaptureTarget: Send + Sync + 'static {
    /// The games to watch.
    fn games(&self) -> Result<Vec<FullGame>>;
    fn capture(&self, game_id: i32) -> Result<CheckpointOutcome>;
    /// Stores a screenshot as the version's.
    fn attach_screenshot(&self, version_id: i32, jpeg: &[u8]) -> Result<()>;
}

pub type NotificationReceiver = mpsc::Receiver<Notification>;

#[derive(Debug, Clone, PartialEq)]
pub enum Notification {
    ChangeDetected {
        game_id: i32,
    },
    VersionCreated {
        game_id: i32,
        version_number: i32,
    },
    /// A version got its screenshot, usually a little after it was created.
    ScreenshotAttached {
        game_id: i32,
        version_id: i32,
    },
    Error(String),
    SyncStarted,
    SyncCompleted(SyncReport),
    SyncFailed(Failure),
    HistoryPruned,
    PruningFailed(Failure),
    // A process runs from the game's install dir, and when its last one exits.
    GameStarted {
        game_id: i32,
    },
    GameStopped {
        game_id: i32,
    },
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
