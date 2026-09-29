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

The CLI also supports `history inspect`, `history export`, and `history import --map UUID=/absolute/path`. Run `cargo run -p cli -- history --help` for the exact arguments. `sync configure|status|adopt`, `devices list|remove` and `prune settings|preview|run` cover sync and history cleanup.

Sync is automatic: while the app or `watch` runs, new versions reach the sync folder as they are created, other devices' progress is picked up every five minutes, and pruning runs when due. `sync` with no subcommand is the manual fallback for when nothing runs in the background. Only a divergence (two devices moved the same slot) needs you; it is reported, never auto-resolved.

## Storage and recovering files by hand

History lives in `repository/` inside the app data directory; the SQLite database next to it is only an index (`repo reindex` rebuilds it). Each checkpoint is a `metadata.json` under `repository/games/<game>/slots/<slot>/versions/<version>/`. Its `files` list gives, for each saved file, its path, its SHA-256 and the `chunks` that rebuild it. Every chunk is stored once, zstd-compressed, at `repository/chunks/<first 2 hex digits>/<remaining 62>`.

Without the app, a file is rebuilt by decompressing its chunks in order and concatenating them:

```sh
for chunk in <hashes from "chunks", in order>; do
  hex=${chunk#sha256:}
  zstd -d -c "repository/chunks/${hex:0:2}/${hex:2}"
done > save.dat
sha256sum save.dat   # compare with the file's "sha256"
```

`cargo run -p cli -- repo extract <version-id> <folder>` does the same for a whole checkpoint, and `repo check --deep` verifies every stored chunk.

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
