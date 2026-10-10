# AGENTS.md

This file provides guidance to coding agents working in `gui/src-tauri/`.

## Scope

This crate is the Tauri backend for the GUI. It should stay thin and primarily translate frontend requests into `worldline-core` calls.

## Responsibilities

- define Tauri commands and event emission
- bridge app state to `WorldlineCore`
- convert errors into Tauri-compatible responses
- avoid duplicating persistence, detection, or restore logic

## Layout

- `src/lib.rs` - app setup: plugins, state, tray, overlay and command registration
- `src/commands.rs` - the Tauri commands called by `../src/ipc/commands.ts`
- `src/tray.rs` - the tray icon and menu
- `src/dialogs.rs` - native dialogs behind commands, so the `e2e` build can stub them
- `src/webview_memory.rs` - lowers WebView2's memory target while a window is hidden
- `src/overlay/` - the in-game overlay: gate, popup state, timers, placement, sound and hotkeys are all decided here; the overlay webview only renders what it is sent

## Commands

Run from `gui/src-tauri/` unless noted otherwise.

```bash
cargo test
cargo clippy
```

From the GUI root:

```bash
cd ..
bun run tauri dev
```

## Implementation Rules

- Keep command handlers short and direct.
- Put reusable behavior in `../../worldline-core` before expanding backend-specific code.
- Preserve the existing event-driven update flow between Rust and the frontend.
- When changing backend behavior, check the matching frontend API wrapper in `../src/`.

## Testing Notes

- Prefer crate-level tests for backend-only logic.
- If a change affects live GUI behavior, verify it with `bun run tauri dev` from `gui/`.
