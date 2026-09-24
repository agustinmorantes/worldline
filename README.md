# Savegame Timeline

Savegame Timeline keeps a history of game save folders. It watches tracked folders, creates checkpoints, and lets you browse branches and restore an earlier state. The desktop app and CLI use the same local repository. Optional sync copies history through a user-selected shared folder while preserving divergent progress.

## Build and run

Install a current Rust toolchain and [Bun](https://bun.sh/). On Linux, install the [Tauri v2 system prerequisites](https://v2.tauri.app/start/prerequisites/) for your distribution, including WebKitGTK 4.1 and the appindicator development library.

```sh
cd gui
bun install --frozen-lockfile
bun run tauri dev
```

Build an installer or Linux package with `bun run tauri build` from `gui/`. Tauri places the result under `target/release/bundle/` (or the configured Cargo target directory). The GUI targets Windows and Linux; Linux packages should be built on Linux.

The CLI can be run from the repository root:

```sh
cargo run -p cli -- games list
cargo run -p cli -- --help
```

Use `--data-dir <path>` before the command to keep a test repository separate from your normal app data. Without it, the CLI uses the desktop app's local data directory.

## Using the app

Add a game and choose its save folders, or use the installed-game import screen to discover games. Checkpoints are created when monitored save files change. The timeline shows slot histories and branches; restoring a checkpoint writes its saved files to the tracked folders, so inspect the selected version first.

In Settings, choose a shared folder to sync between devices. Each device keeps its own active slot and version pointers. History cleanup can retire eligible checkpoints while preserving graph links. Export History on a game's detail screen creates a portable directory bundle; Import History in Settings inspects a bundle and asks where each tracked save folder belongs on this machine. Importing history does not restore live save files.

The CLI also supports `history inspect`, `history export`, and `history import --map UUID=/absolute/path`. Run `cargo run -p cli -- history --help` for the exact arguments.

## Development

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd gui
bun run check
bun run test -- --run
bun run build
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the workspace layout and contribution process.

## License

[MIT](LICENSE).
