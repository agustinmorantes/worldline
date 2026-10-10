# Architecture

Worldline is a Rust workspace with a React desktop app on [Tauri v2](https://v2.tauri.app/).

```
┌──────────────────────┐   ┌──────────────┐
│ gui/src  (React, TS) │   │     cli/     │
├──────────────────────┤   │   (clap)     │
│ gui/src-tauri (Rust) │   │              │
└──────────┬───────────┘   └──────┬───────┘
           └──────────┬───────────┘
              ┌───────▼────────┐
              │ worldline-core │  history, capture, restore, sync, pruning, bundles
              └───────┬────────┘
       ┌──────────────┼──────────────────┐
  repository/     SQLite index      sync folder (another store)
  (the truth)     (rebuildable)
```

- **`worldline-core/`** owns all behavior: the on-disk store, capture, restore, sync, pruning, export and import, and game discovery. `WorldlineCore` is its facade.
- **`gui/src-tauri/`** and **`cli/`** are thin adapters over `WorldlineCore`. The Tauri side adds the tray, the in-game overlay and the IPC commands.
- **`gui/src/`** is the React UI, organized by feature (`library`, `sessions`, `graph`, `add-games`, `settings`, …). Its IPC types are generated from the Rust models with [ts-rs](https://github.com/Aleph-Alpha/ts-rs).

## Main ideas

- **Files are the truth.** History is files in a store; SQLite is only an index plus device-local facts ([ADR 0001](adr/0001-repository-files-are-truth-sqlite-is-index.md), [ADR 0004](adr/0004-sqlite-stays-as-a-uuid-keyed-index.md)). The local repository, the sync folder and an export bundle share one [format](storage-format.md).
- **Content-addressed chunks.** Saves are split into chunks with FastCDC, compressed with zstd and stored once ([ADR 0003](adr/0003-chunking-and-zstd-instead-of-delta-chains.md)).
- **Capture.** A file watcher feeds a scheduler that waits for the game to finish writing, then checkpoints at most once per cooldown. Nothing is captured when the content is unchanged.
- **Restore never leaves saves half-written.** Every write to live saves is a journaled transaction that ends in the state before or after, never in between, and checkpoints the current saves first ([ADR 0008](adr/0008-live-saves-change-atomically-or-not-at-all.md)).
- **Sync through any shared folder.** Devices exchange history through a folder that Dropbox, OneDrive, Syncthing or a network drive keeps in step. There is no server and no lock: each device publishes its own pointers, and when two devices played the same game, both lines are kept as branches. Pruning the shared folder is safe with several devices ([ADR 0002](adr/0002-multi-device-remote-gc-by-condemn-and-sweep.md)).
- **The overlay stays out of the game.** The in-game popup is a separate window, never injected ([ADR 0009](adr/0009-game-overlay-is-a-separate-window-driven-from-rust.md)).
- **The GUI owns its words.** All user-facing text, errors included, lives in the GUI's i18n files ([ADR 0007](adr/0007-gui-owns-its-copy-errors-and-language.md)).

Terms like *version*, *slot*, *branch* and *remote* are defined in the [glossary](glossary.md).
