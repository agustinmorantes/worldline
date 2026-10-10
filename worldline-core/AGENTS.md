# AGENTS.md

This file provides guidance to coding agents working in `worldline-core/`.

## Crate Role

`worldline-core` is the shared Rust library for the workspace; the Tauri app and the CLI are thin adapters over `WorldlineCore` (`WorldlineCore::open(data_dir, OpenOptions)`).

## Layout

- `src/store.rs` - the on-disk history format shared by the local repository, the sync remote and bundles; every validator lives here
- `src/chunks.rs` - FastCDC + zstd capture and chunk reads
- `src/history.rs` - the local history store, the only writer of local history
- `src/index.rs` - SQLite index projected from the files, plus device-local pointers
- `src/transfer.rs` - store-to-store copy used by sync, export and import
- `src/sync.rs`, `src/maintenance.rs`, `src/bundle.rs`, `src/integrity.rs`, `src/restore.rs`
- `src/sync_loop.rs` - background thread started by `open_shared`: full sync + due pruning on start and every five minutes, a push after each created version, a final push on `shutdown`
- `src/detection/` - capture scheduler and screenshot timing (pure, fake-clock tested), the notify watcher adapter, running-game detection, and the screenshot worker thread
- `src/screenshot.rs` - window capture through Windows.Graphics.Capture (`screenshot_unsupported.rs` on other platforms)
- `src/core_facade.rs` - `WorldlineCore`, including guarded mutation and the sync refresh flow
- `src/install_scanner.rs` - installed-game scanning and manifest handling
- `migrations/` - one squashed initial migration

## Design Rules

- Repository files are the truth; SQLite is an index (ADR 0001, 0004). A mutation writes files first, then projects them into the index (`History::refresh_game`). `reindex` must always be able to rebuild everything except device-local columns (active pointers, icon) and scan roots.
- Every write to live saves runs inside `WorldlineCore::exclusive` (suppress capture → lock → reload the watcher afterwards) and preserves the live saves first. `guarded` does that by checkpointing before applying; flows that must pull before they know where progress belongs (adoption, a remotely deleted active slot) capture and compare themselves, and never overwrite content they haven't recorded or shown the other device saw.
- Live saves change atomically or not at all (ADR 0008). Every write to live saves goes through `restore::apply`'s transaction; never delete or truncate a live file outside it, and never put anything of ours in a game's folders.
- Everything that writes chunks or deletes them (capture, pull, import, GC) holds `store::with_lock`. The lock is not reentrant: take it once, at the outermost call.
- Missing remote data is *not arrived*, never an error that aborts a whole sync; a stray file never blocks an operation.
- Use `anyhow::Result` plus `.context(...)` for fallible operations.

## Testing Guidance

- Integration tests in `tests/` drive `WorldlineCore` over temp dirs; `tests/common` has the `Device` harness, and `tests/sync.rs` runs several devices on one remote. Adopted games there share the same save folder, so set live saves right before each device acts.
- `tests/storage_ratio.rs` measures storage efficiency; run it with `--nocapture` to see the ratios.
- Windows paths are limited to 260 characters: keep temp names and slugs short.

## Install Scanner Notes

- `install_scanner.rs` is substantial and performance-sensitive. Prefer local helper functions and targeted tests over broad rewrites.
- The scanner can refresh the Ludusavi manifest over HTTP, but it must keep working with the bundled fallback manifest in `assets/manifest.yaml`.
- Cross-platform path resolution matters here. Check Windows-specific branches carefully before simplifying path handling.
