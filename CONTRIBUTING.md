# Contributing

Worldline is a Rust workspace with a React/Tauri desktop app. `worldline-core/` owns persistence, watching, snapshots, sync, and history transfer. `cli/` and `gui/src-tauri/` should stay thin adapters to the core. The frontend lives in `gui/src/`. [docs/architecture.md](docs/architecture.md) gives the overview, the [glossary](docs/glossary.md) the domain terms, and the [decision records](docs/adr/) the reasons behind the design. A change that reverses one of those decisions should update or supersede its record.

Before sending a change, read the relevant `AGENTS.md` in the area you touch and run the focused tests for that area. The full checks are in the root [README.md](README.md#development). On Linux, install the [Tauri v2 prerequisites](https://v2.tauri.app/start/prerequisites/) before running workspace checks or building packages.

Pushes to `main`/`dev` and pull requests run one Linux job: core/CLI tests and Clippy, Rust formatting, GUI lint/type checks and unit tests, and IPC binding verification. Desktop builds, Windows checks, screen tests, and native E2E run for `v*` release tags and must pass before bundles and the draft release are created. Manual bundle builds run the inexpensive checks before packaging. Run the full checks locally when changing the desktop adapter or platform-specific behavior.

Open an issue for a bug or proposed behavior change, then submit a pull request with the problem, the change, and how you verified it. Add a test when behavior changes. Keep user data and save files out of issues, logs, and test fixtures. Do not use a real save repository for destructive tests; the CLI accepts `--data-dir` and the Rust tests use temporary directories.
