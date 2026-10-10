---
status: accepted
date: 2026-09-27
---

# Repository files are the source of truth; SQLite is a rebuildable index

History lives in the store's files: `metadata.json` for each game, slot, tracked directory and version, plus the chunks. Only the history store module writes them. SQLite holds just two things: an index that can be rebuilt from the files, and facts that belong to one device (active pointers, settings, scan roots). A crash therefore leaves an index to rebuild, never two authorities that disagree.

We chose this because sync, export/import and recovering saves by hand without the database already depend on the files. A review of the earlier design also traced most of its serious bugs to SQLite and the files being written separately at every call site: orphaned imports, deleted slots resurrected by sync, UNIQUE collisions on adoption, and folder-name drift.

## Considered options

- **SQLite authoritative, files as an export.** Rejected: sync and hand recovery would depend on a derived copy, and every mutation would still write both.
- **Both authoritative, with a careful write order** (the earlier design). Rejected: the ordering rules end up spread across every caller, and that spread is where the bugs came from.

## Consequences

- Deletion is a tombstone in the files, not a row delete, so other devices don't resurrect deleted data.
- The local repository, the remote and a bundle share one on-disk format (see the [glossary](../glossary.md#storage): Store).
- Whether SQLite is needed at all is decided in [ADR 0004](0004-sqlite-stays-as-a-uuid-keyed-index.md).
