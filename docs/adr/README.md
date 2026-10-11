# Architecture decision records

Each record states a decision, the options it was weighed against, and what follows from it. Terms are defined in the [glossary](../glossary.md).

| ADR | Decision |
|---|---|
| [0001](0001-repository-files-are-truth-sqlite-is-index.md) | Repository files are the source of truth; SQLite is a rebuildable index |
| [0002](0002-multi-device-remote-gc-by-condemn-and-sweep.md) | Remote garbage collection with multiple devices: condemn, sweep after a grace period, publishers heal |
| [0003](0003-chunking-and-zstd-instead-of-delta-chains.md) | Space efficiency through content-defined chunking and zstd, not delta chains |
| [0004](0004-sqlite-stays-as-a-uuid-keyed-index.md) | SQLite stays, as a UUID-keyed index that sync can never violate |
| [0005](0005-game-icon-and-accent-live-in-the-remote.md) | Game icon and accent color live in the remote |
| [0006](0006-untrack-is-device-local-delete-is-a-tombstone.md) | Untracking a game is device-local; deleting it is a tombstone |
| [0007](0007-gui-owns-its-copy-errors-and-language.md) | The GUI owns its copy, its error messages and the language |
| [0008](0008-live-saves-change-atomically-or-not-at-all.md) | Live saves change atomically or not at all |
| [0009](0009-game-overlay-is-a-separate-window-driven-from-rust.md) | The game overlay is a separate window, driven from Rust |
| [0011](0011-deleted-games-and-slots-stay-recoverable-for-30-days.md) | Deleted games and slots stay recoverable for 30 days |
