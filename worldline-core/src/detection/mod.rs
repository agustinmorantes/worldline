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
use chrono::NaiveDateTime;
use serde::Serialize;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

pub use running::{RunningGames, runs_from};
pub use watcher::{Checkpoints, Watcher};

/// What the watcher needs from the core.
pub trait CaptureTarget: Send + Sync + 'static {
    /// The games to watch.
    fn games(&self) -> Result<Vec<FullGame>>;
    /// The outcome, and where the new version branches off, if it does.
    fn capture(&self, game_id: i32) -> Result<(CheckpointOutcome, Option<BranchFrom>)>;
    /// Stores a screenshot as the version's.
    fn attach_screenshot(&self, version_id: i32, jpeg: &[u8]) -> Result<()>;
    /// Records whether the game's live saves differ from its active version;
    /// `due_at`: when the watcher will capture them.
    fn check_unrecorded(&self, game_id: i32, due_at: NaiveDateTime) -> Result<()>;
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
        /// Set when the version starts a new branch: its parent already had
        /// another child in the slot (the first capture after a restore).
        branch_from: Option<BranchFrom>,
    },
    /// A version got its screenshot, usually a little after it was created.
    ScreenshotAttached {
        game_id: i32,
        version_id: i32,
    },
    Error(String),
    /// An automatic capture failed; also reported as `Error` with the details.
    CheckpointFailed {
        game_id: i32,
        reason: CaptureFailure,
    },
    SyncStarted,
    SyncCompleted(SyncReport),
    SyncFailed(Failure),
    HistoryPruned,
    PruningFailed(Failure),
    /// Games changed outside a sync, like a missing icon filled from an install.
    GamesChanged,
    // A process runs from the game's install dir, and when its last one exits.
    GameStarted {
        game_id: i32,
    },
    GameStopped {
        game_id: i32,
    },
    /// The game's live saves differ from its active version, and the watcher
    /// captures them at `due_at`; `None` once they no longer differ. Sent
    /// only when that changes.
    UnrecordedChanges {
        game_id: i32,
        due_at: Option<NaiveDateTime>,
    },
}

/// The parent of a version that starts a new branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct BranchFrom {
    pub version_number: i32,
    pub tag: Option<String>,
}

/// Why an automatic capture failed, as far as a front end tells them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum CaptureFailure {
    DiskFull,
    /// The saves hold far more than usual (`ErrorKind::TooLarge`).
    TooLarge,
    Other,
}

/// SQLite's message for `SQLITE_FULL`, which Diesel reports without its code.
const SQLITE_FULL: &str = "database or disk is full";

impl From<&anyhow::Error> for CaptureFailure {
    fn from(error: &anyhow::Error) -> Self {
        if crate::error::error_kind(error) == Some(crate::error::ErrorKind::TooLarge) {
            return Self::TooLarge;
        }
        let full = error.chain().any(|cause| {
            cause
                .downcast_ref::<std::io::Error>()
                .is_some_and(|io| io.kind() == std::io::ErrorKind::StorageFull)
                || matches!(
                    cause.downcast_ref::<diesel::result::Error>(),
                    Some(diesel::result::Error::DatabaseError(_, info)) if info.message() == SQLITE_FULL
                )
        });
        if full { Self::DiskFull } else { Self::Other }
    }
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

#[cfg(test)]
mod tests {
    use super::CaptureFailure;
    use anyhow::Context;
    use std::io;

    fn failure(error: io::Error) -> CaptureFailure {
        let result: io::Result<()> = Err(error);
        CaptureFailure::from(&result.context("failed to create chunk").unwrap_err())
    }

    #[test]
    fn a_full_disk_anywhere_in_the_chain_is_disk_full() {
        assert_eq!(
            failure(io::ErrorKind::StorageFull.into()),
            CaptureFailure::DiskFull
        );
        #[cfg(windows)]
        for code in [39, 112] {
            // ERROR_HANDLE_DISK_FULL, ERROR_DISK_FULL
            assert_eq!(
                failure(io::Error::from_raw_os_error(code)),
                CaptureFailure::DiskFull
            );
        }
        #[cfg(unix)]
        assert_eq!(
            failure(io::Error::from_raw_os_error(28)), // ENOSPC
            CaptureFailure::DiskFull
        );
    }

    /// Runs `sql` on a real SQLite database that can't grow past two pages.
    fn sqlite_failure(sql: &str) -> CaptureFailure {
        use diesel::{Connection, RunQueryDsl, SqliteConnection, sql_query};
        let mut conn = SqliteConnection::establish(":memory:").unwrap();
        sql_query("PRAGMA max_page_count = 2")
            .execute(&mut conn)
            .unwrap();
        sql_query("CREATE TABLE t (v BLOB)")
            .execute(&mut conn)
            .unwrap();
        let result: anyhow::Result<usize> = sql_query(sql).execute(&mut conn).map_err(Into::into);
        CaptureFailure::from(&result.context("failed to project checkpoint").unwrap_err())
    }

    #[test]
    fn a_full_sqlite_database_is_disk_full() {
        assert_eq!(
            sqlite_failure("INSERT INTO t VALUES (zeroblob(100000))"),
            CaptureFailure::DiskFull
        );
    }

    #[test]
    fn other_database_errors_are_other() {
        assert_eq!(
            sqlite_failure("INSERT INTO missing VALUES (1)"),
            CaptureFailure::Other
        );
    }

    #[test]
    fn other_write_errors_are_other() {
        assert_eq!(
            failure(io::ErrorKind::PermissionDenied.into()),
            CaptureFailure::Other
        );
        assert_eq!(
            CaptureFailure::from(&anyhow::anyhow!("game has no active slot")),
            CaptureFailure::Other
        );
    }
}
