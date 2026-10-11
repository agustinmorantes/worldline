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
    /// A write to live saves failed and couldn't be fully undone yet; the
    /// core finishes undoing it by itself, and the checkpoint taken before
    /// it holds the saves meanwhile.
    RestoreIncomplete,
    /// Another game in the library already has this name.
    NameTaken,
    /// Another game already tracks this folder, or one inside or around it.
    FolderTaken,
    /// The folder holds other apps' files too (the home folder, AppData...).
    SharedFolder,
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::SyncFolderUnreachable => "Sync folder is unavailable",
            Self::GameRunning => "The game is running",
            Self::SavesInUse => "The game is using its save files",
            Self::RestoreIncomplete => "The saves couldn't be fully put back",
            Self::NameTaken => "Another game has this name",
            Self::FolderTaken => "Another game tracks this folder",
            Self::SharedFolder => "Other apps keep files in this folder",
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

/// Tags a sharing or lock violation anywhere in `error`'s chain (the game holds
/// the file) as `SavesInUse`.
pub fn tag_in_use(error: anyhow::Error) -> anyhow::Error {
    let held = cfg!(windows)
        && error.chain().any(|cause| {
            cause
                .downcast_ref::<std::io::Error>()
                .is_some_and(|io| matches!(io.raw_os_error(), Some(32 | 33)))
        });
    if held {
        error.context(ErrorKind::SavesInUse)
    } else {
        error
    }
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
