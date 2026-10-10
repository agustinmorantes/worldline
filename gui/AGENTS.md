# AGENTS.md

This file provides guidance to coding agents working in `gui/`.

## Scope

`gui/` contains the desktop application shell:

- `src/` - React 19 frontend: `app/` (providers, shell, TanStack routes), `features/` (one folder per screen or flow), `shared/` (UI primitives, i18n, tokens), `ipc/` (Tauri command and event wrappers plus generated bindings), and `overlay/` (the in-game overlay window).
- `src-tauri/` - Tauri v2 backend crate that exposes `worldline-core` through commands and events.

Keep this layer focused on presentation, interaction flow, and Tauri integration. Shared business logic belongs in `../worldline-core`.

## Commands

Run from `gui/` unless noted otherwise.

```bash
bun run tauri dev
bun run build
bun run check
bun run test
bun run ci
```

Useful backend-only checks:

```bash
cd src-tauri
cargo clippy
cargo test
```

## Frontend Guidance

- Use the existing stack: React 19, TypeScript, TanStack Router, TanStack Query, Tailwind CSS v4, Radix primitives, and Tauri APIs.
- Call the backend through `src/ipc/commands.ts` and `src/ipc/events.ts` instead of invoking Tauri directly from components.
- The types in `src/ipc/bindings/` are generated from `worldline-core`; regenerate them with `bun run bindings` instead of editing them by hand.
- Preserve the current visual language unless the task is explicitly a design refresh.
- Keep routes in `src/app/routes/` thin, put screen logic in `src/features/`, and put reusable UI in `src/shared/ui/`.

## Tauri Guidance

- Keep commands thin. They should mostly validate inputs, call `WorldlineCore`, and convert errors into Tauri-friendly return values.
- Emit and consume Tauri events for live updates instead of adding polling.
- Do not duplicate persistence or snapshot logic in `src-tauri`; put reusable behavior in `worldline-core` first.

## Testing And Validation

- For frontend-only changes, run `bun run check` and any targeted `bun run test` coverage available.
- For Tauri/backend bridge changes, also run `cargo test` or `cargo clippy` in `src-tauri` as needed.
- For end-to-end behavior affecting saves, versions, or watcher events, validate through `bun run tauri dev`.
- E2E tests (`bun run e2e:build`, then `bun run e2e`): they need an msedgedriver matching the installed WebView2, either on PATH or passed with `TAURI_NATIVE_DRIVER=<path to msedgedriver> bun run e2e`.

## Integration Notes

- The frontend package scripts are defined in `package.json`.
- `bun run ci` currently runs ESLint with fixes, TypeScript checking, and `cargo clippy` in `src-tauri`.
- The Tauri crate depends on `../../worldline-core`; when backend command behavior grows, move it into core instead of expanding adapter code.
