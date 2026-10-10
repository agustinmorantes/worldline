# Contributing

Worldline is a Rust workspace with a React/Tauri desktop app. `worldline-core/` owns persistence, watching, snapshots, sync, and history transfer. `cli/` and `gui/src-tauri/` should stay thin adapters to the core. The frontend lives in `gui/src/`. [docs/architecture.md](docs/architecture.md) gives the overview, the [glossary](docs/glossary.md) the domain terms, and the [decision records](docs/adr/) the reasons behind the design. A change that reverses one of those decisions should update or supersede its record.

Before sending a change, read the relevant `AGENTS.md` in the area you touch and run the focused tests for that area. The full checks are in the root [README.md](README.md#development). On Linux, install the [Tauri v2 prerequisites](https://v2.tauri.app/start/prerequisites/) before running workspace checks or building packages.

Open an issue for a bug or proposed behavior change, then submit a pull request with the problem, the change, and how you verified it. Add a test when behavior changes. Keep user data and save files out of issues, logs, and test fixtures. Do not use a real save repository for destructive tests; the CLI accepts `--data-dir` and the Rust tests use temporary directories.
