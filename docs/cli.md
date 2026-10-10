# Command-line interface

The CLI works on the same local repository as the desktop app. Run it from the repository root:

```sh
cargo run -p cli -- --help
cargo run -p cli -- <command> --help
```

Put `--data-dir <path>` before the command to use a separate repository, for example for testing. Without it, the CLI uses the app's [data directory](storage-format.md#where-it-lives).

| Command | What it does |
|---|---|
| `games list \| show \| add \| edit` | Manage tracked games. `games add "Hades" --dir <save folder>` starts tracking one. |
| `games untrack` | Stop tracking a game on this device. Its history stays in the sync folder. |
| `games delete` | Delete a game and its history on every device. |
| `versions checkpoint` | Capture the live saves now. Does nothing when they haven't changed. |
| `versions tag \| restore` | Name a checkpoint, or write it back into the live save folders. |
| `slots list \| create \| switch \| rename \| delete` | Manage playthroughs: independent histories of one game. |
| `watch` | Watch every tracked game and create checkpoints as they save. Syncs in the background. |
| `sync` | Sync once with the sync folder. Only needed when neither the app nor `watch` is running. |
| `sync status \| configure \| adopt` | Show sync state, set the sync folder (`--off` turns sync off), or start tracking a game from it. |
| `devices list \| set \| remove` | Show the devices sharing the sync folder, rename this device, or stop protecting one's history. |
| `prune settings \| preview \| run` | Retention settings, and what cleaning up history would remove. |
| `history inspect \| export \| import` | Portable `.zip` bundles of one game's history. `import --map UUID=/absolute/path` says where each save folder belongs on this machine. Importing never touches live saves. |
| `repo check [--deep]` | Verify manifests and chunks. `--deep` decompresses every chunk. |
| `repo extract <version-id> <folder>` | Rebuild a checkpoint's files into a folder, without touching live saves. |
| `repo reindex` | Rebuild the database index from the repository files. |
