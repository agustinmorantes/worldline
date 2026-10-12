---
status: accepted
date: 2026-10-12
---

# A full sync reads and pushes only the games that changed

A full sync used to read every game's metadata from the remote (every game, slot and version `metadata.json`) and push every local game, even when nothing had changed. On a cloud-synced folder each of those reads is a file the sync client may have to fetch, so an idle periodic sync grew with the size of the whole history.

## Decision

- **Devices publish a stamp per game.** `devices/<device>/state.json` gains an optional `stamps` field: for each game the device pushed, a `digest` and what the push put on the remote. The digest hashes the game as the device holds it, its retirement markers and which of its versions have screenshots. The stamp also lists the game's and each slot's and tracked folder's `updated_at`, the tip versions (versions nothing descends from), the versions edited since they were created, at their `updated_at`, the version UUIDs with activity (`last_played_at`) and the unretired version UUIDs whose screenshots were pushed, and sticky game/slot deletions with the versions those slot deletions observed. Older clients ignore the field, and a state without it still reads. The store version stays the same.
- **A device keeps what it last read from the remote.** The cache lives in `cache/sync/` in the data directory. `remote.json` holds the remote's folder, its store's `created_at`, the remote's retirement markers, and, per game, the digests it was read at, whether everything those stamps describe had arrived, whether it was fully pulled, and the digest this device last pushed. `games/<uuid>.json` holds the record read. The cache is derived and device-local, and is never synced.
- **A full sync reads a game only when needed.** It still lists `games/` and `retired-versions/` and reads every device's `state.json`. It reads a game's metadata when:
  - its folder is new to the cache;
  - any device's stamp for it changed;
  - it hadn't fully arrived when last read.
  Retirement markers are write-once, so only names not seen before are read.
- **Arrival is checked against the stamps.** A game has arrived when the remote holds everything every stamp describes. That means the game, every slot and tracked folder, and every edited version, each at least as new as the stamp. It also means every tip, with its parents reaching a root, and activity on each advertised version: activity is only ever filled in, never changed. Advertised sticky deletions and each deleted slot's observed versions must be present too; a newer metadata timestamp alone cannot acknowledge them. A game that hasn't arrived is read again on every sync until it has, even if its writer publishes nothing new. Cloud folders deliver files in any order, so a state can arrive before the metadata it describes.
- **A full sync pulls a game when needed.** That is when the game was read, gained a marker, or its last pull left something missing: a version not arrived, a missing chunk, an icon not arrived, an error, or, when screenshots sync, an advertised unretired screenshot still missing. A marker first read by a push still has its game pulled by the next sync. A game that was linked or adopted is pulled again on the next sync, and every game is pulled again once screenshot sync is turned on.
- **A full push skips games this device already pushed.** A game is skipped when its digest equals the one this device last pushed and the remote copy still holds all of it. Other games are copied, then read back. A pushed game's stamp is published only if everything it describes is there. Otherwise the next push copies it again. A skipped game still has its chunks healed from its cached manifests by bulk presence checks. The daily full heal checks every one. The recent heal checks versions from the last six days (twice the grace period) and an icon newer than that. Scoped pushes (a checkpoint, retention) are unchanged and publish stamps too.
- **Every day, a sync reads everything.** That sync trusts nothing: when the last check is a day old, the cache is emptied, so every game is read and pushed again. Every sync also reads every game while some device publishes no stamps (an older client) or a `state.json` can't be read; what this device pushed is still known, so its pushes keep skipping unchanged games. A cache that is missing, unreadable, from another cache format, or made for another folder or another store (`created_at`) is replaced by an empty one, and the sync reads everything.
- **The cache is written only after the remote.** Record files are written before `remote.json`, which comes after the device's state is published. A crash between them leaves an older cache, which only makes the next sync do more. While a sync runs it holds the cache. A sync that fails keeps no in-memory copy, so the next sync starts from the copy on disk.

Recovery and purge marks from [ADR 0011](0011-deleted-games-and-slots-stay-recoverable-for-30-days.md) are advertised independently of metadata timestamps. Pending recovery uploads bypass the unchanged-game shortcut and heal all history until verified; pending recovery pulls continue even while a game waits for adoption. Cache healing excludes expired or purged history.

Pointers, protected versions, grace periods, fail-closed garbage collection, the sync lock and guarded live-save writes are unchanged ([ADR 0002](0002-multi-device-remote-gc-by-condemn-and-sweep.md), [ADR 0008](0008-live-saves-change-atomically-or-not-at-all.md)).

## Considered options

- **File modification times or sizes instead of stamps.** Rejected: cloud clients rewrite times on download, and a time doesn't say whether the files it describes have arrived.
- **A remote-wide change log.** Rejected: every device appending to one file conflicts in cloud folders. A stamp lives in the state file each device already owns.
- **Trusting stamps forever.** Rejected: a file lost or reverted behind every device's back changes no stamp. The daily check bounds how long that goes unnoticed.
- **A store version bump.** Rejected: older clients can keep syncing with the same remote. They only make newer ones read everything.

## Consequences

- An idle sync's remote reads are the two folder listings, one `state.json` per device and any new marker. Before this change it also read the metadata of every game. Its local reads still cover every game: stamps are computed from the local index, and each version is checked for a screenshot.
- What a stamp identifies stays pending until it arrives, without its writer publishing again: a screenshot or activity that reaches the reader after the state is pulled again on the next sync. Only what no stamp signals waits for the daily check: metadata or screenshots lost on the remote, or a screenshot changed without changing its writer's stamp.
- A game whose stamp describes something that never arrives (lost on the writer's side) is read on every sync until the daily check republishes it.
- Retention, remote garbage collection and the sync status still read every game when they run.
