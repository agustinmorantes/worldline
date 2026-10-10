---
status: accepted
date: 2026-09-27
---

# SQLite stays, as a UUID-keyed index that sync can never violate

With the files authoritative, an in-memory index built at startup could replace SQLite. We keep SQLite, in the narrow role ADR 0001 gives it.

- The GUI and CLI use integer ids. Upserting rows by UUID keeps those ids stable across reindexing, which an in-memory index rebuilt on every sync would not.
- Device-local facts (active slot and version pointers, icons, scan roots) need durable storage anyway.
- The only DB-level uniqueness is UUID. Title, slot-name and tracked-path uniqueness are enforced by the history store on local edits only. Data arriving from another device can therefore never fail an insert: a title or path collision is detected before transfer and reported, or linked when it is the same game.

## Considered options

- **In-memory index built at startup.** Rejected: ids would change on every rebuild, and device-local facts would need a new file format.
- **SQLite with the old UNIQUE constraints.** Rejected: adoption crashed on them when another device had a game with the same title.
