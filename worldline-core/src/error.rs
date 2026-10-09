//! The coarse error kinds a front end can tell apart (ADR 0007). Every other
//! failure is just an error: front ends show their own message for the call.

use serde::Serialize;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum ErrorKind {
    /// The sync folder is missing or can't be read (an unplugged drive, an offline share).
    SyncFolderUnreachable,
    /// The operation needs the game closed.
    GameRunning,
    /// The game holds a save file; nothing was changed.
    SavesInUse,
    /// A write to live saves failed and couldn't be fully undone; the
    /// checkpoint taken before it holds the saves.
    RestoreIncomplete,
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::SyncFolderUnreachable => "Sync folder is unavailable",
            Self::GameRunning => "The game is running",
            Self::SavesInUse => "The game is using its save files",
            Self::RestoreIncomplete => "The saves couldn't be fully put back",
        })
    }
}

impl std::error::Error for ErrorKind {}

/// The kind attached anywhere in `error`'s chain, as a context or a cause.
pub fn error_kind(error: &anyhow::Error) -> Option<ErrorKind> {
    error
        .downcast_ref::<ErrorKind>()
        .or_else(|| error.chain().find_map(|cause| cause.downcast_ref()))
        .copied()
}

/// A failure as front ends receive it: the whole chain for the log, and its kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Failure {
    pub message: String,
    pub kind: Option<ErrorKind>,
}

impl From<&anyhow::Error> for Failure {
    fn from(error: &anyhow::Error) -> Self {
        Self {
            message: format!("{error:#}"),
            kind: error_kind(error),
        }
    }
}
