# Storage format and recovering files by hand

Worldline's history is plain files on disk. You can get your saves back without the app, the CLI or the database.

## Where it lives

The app and the CLI share one data directory:

| Platform | Data directory |
|---|---|
| Windows | `%LOCALAPPDATA%\agustinmorantes.worldline` |
| Linux | `~/.local/share/agustinmorantes.worldline` |

Pass `--data-dir <path>` to the CLI to use another one.

History lives in `repository/` inside that directory. The SQLite database next to it is only an index: `repo reindex` rebuilds it from the files ([ADR 0001](adr/0001-repository-files-are-truth-sqlite-is-index.md)). The sync folder and exported bundles use the same layout.

## Layout

```
repository/
├── games/<game>/
│   ├── metadata.json
│   ├── icon.png
│   └── slots/<slot>/
│       ├── metadata.json
│       └── versions/<version>/
│           ├── metadata.json        # the checkpoint's manifest
│           └── screenshot.jpg       # optional
└── chunks/<first 2 hex digits>/<remaining 62>
```

Each checkpoint is a `metadata.json` whose `files` list gives, for each saved file, its path, its SHA-256 and the `chunks` that rebuild it. Every chunk is stored once, zstd-compressed, and named after the SHA-256 of its uncompressed bytes. Files above about 256 KiB are split with content-defined chunking, so a small edit to a large save only stores the chunks that changed ([ADR 0003](adr/0003-chunking-and-zstd-instead-of-delta-chains.md)).

## Rebuilding a file by hand

Decompress the file's chunks in order and concatenate them:

```sh
for chunk in <hashes from "chunks", in order>; do
  hex=${chunk#sha256:}
  zstd -d -c "repository/chunks/${hex:0:2}/${hex:2}"
done > save.dat
sha256sum save.dat   # compare with the file's "sha256"
```

The CLI does the same for a whole checkpoint, without touching your live saves:

```sh
cargo run -p cli -- repo extract <version-id> <folder>
```

`repo check --deep` verifies every stored chunk.
