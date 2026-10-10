# AGENTS.md

This file provides guidance to coding agents working anywhere in the `worldline` workspace.

## Project Overview

**Worldline** is a Rust-first workspace for tracking game save directories, creating restore points, and restoring previous states. The repository currently ships three user-facing surfaces:

- `worldline-core/` - reusable Rust library with the history store, file watching, sync, retention and install scanning.
- `gui/` - React + Tauri desktop application that exposes `worldline-core` through a thin backend adapter.
- `cli/` - Rust CLI that uses the same core library and data layout as the GUI.

## Workspace Structure

- `worldline-core/`
  Core crate `worldline-core`. This is where business logic belongs.
  See `worldline-core/AGENTS.md` for crate-specific architecture notes.

- `gui/`
  Frontend app and Tauri shell.
  - `gui/src/` contains the React 19 frontend.
  - `gui/src-tauri/` contains the Tauri backend crate and command bridge to `worldline-core`.
  See `gui/AGENTS.md` for frontend and Tauri-specific guidance.

- `cli/`
  Rust CLI crate that shares the same data model and repository layout as the GUI app.
  See `cli/AGENTS.md` for command and testing guidance.

## Essential Commands

Run from the workspace root unless noted otherwise.

```bash
cargo test --workspace
cargo clippy --workspace --all-targets
cargo fmt --all
```

GUI development:

```bash
cd gui
bun run tauri dev
bun run check
bun run test
```

CLI development:

```bash
cargo run -p cli -- --help
cargo test -p cli
```

Core-only development:

```bash
cargo test -p worldline-core
cargo clippy -p worldline-core --all-targets
```

## Architecture

### Shared core

`worldline-core` owns:

- the history store (repository files are the truth; SQLite is a rebuildable index)
- snapshot creation and restore logic
- notify-based background watching
- scan-root management
- installed-game discovery from local paths and store metadata

Keep new business logic in this crate unless it is unavoidably UI-only.

### GUI flow

1. React UI triggers an action.
2. Frontend API wrapper calls a Tauri command.
3. `gui/src-tauri/src/commands.rs` delegates to `WorldlineCore`.
4. `worldline-core` updates persistence and/or watcher state.
5. Tauri emits events back to the frontend for real-time updates.

The Tauri layer should stay thin. If a command needs more than light argument adaptation and error conversion, the logic likely belongs in `worldline-core`.

### CLI flow

The CLI initializes the same repositories and migrations as the GUI, then delegates command behavior to `WorldlineCore`. Keep output formatting and argument parsing in `cli`, but avoid duplicating core logic there.

## Working Rules

- Prefer crate-targeted commands while iterating on a single area; use workspace-wide commands before finishing.
- Do not introduce app-specific business rules directly into `gui/src-tauri` or `cli/src/main.rs` if the behavior should also exist for the other interface.
- When changing the store format or the index schema, update `store.rs`, the migration, `index.rs`, and any CLI/GUI surfaces that depend on them together. Pre-release data is moved aside, not migrated.
- When changing watcher behavior, verify both library tests and at least one end-to-end flow through GUI or CLI.

## Data Layout

Application data is stored under the platform local data directory for the app identifier used by the GUI and CLI. The important runtime artifacts are:

- `database.db` - SQLite index, rebuildable from `repository/`
- `repository/` - the history store: captured versions, chunks and game icons
- `txn/` - in-flight restore transactions, recovered on open
- `cache/` - install scanner manifest and icon caches
- `*_settings.json`, `sync_state.json`, `untracked_games.json` - device-local settings and state

Do not hardcode development-only paths in production code; keep using the existing app-data resolution pattern.

## Common Gotchas

- The workspace mixes Rust 2024 crates (`worldline-core`, `cli`) with a Rust 2021 Tauri crate (`gui/src-tauri`). Keep changes compatible with each crate's edition.
- `gui/node_modules/` and `target/` may exist in the repo checkout; do not treat them as source.
- The install scanner can hit the network to refresh the manifest, but it also has bundled fallback data in `worldline-core/assets/`.
- The root `Cargo.toml` workspace members are `worldline-core`, `gui/src-tauri`, and `cli`.
