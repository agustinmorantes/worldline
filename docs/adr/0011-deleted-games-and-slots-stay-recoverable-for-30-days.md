---
status: accepted
date: 2026-10-11
---

# Deleted games and slots stay recoverable for 30 days

Deleting a game or a slot used to be final. The tombstone was sticky (`deleted_at` merged with `or`), and garbage collection left deleted items out of the live set, so their chunks were condemned at once and swept 72 h later (ADR 0002). One wrong click on any device erased a history everywhere.

A deleted game or slot now stays recoverable for a **recovery window** of 30 days (plan: `docs/delete-and-recover-plan.md`, D2). This supersedes the tombstone meaning of ADR 0006 and amends ADR 0002's live set.

## Decision

- `GameMeta` and `SlotMeta` gain `recovered_at` and `purged_at`. Merging takes the later `deleted_at` and the later `recovered_at`; `purged_at` is sticky. An item is deleted when it is purged or when `deleted_at` is later than `recovered_at`, so deleting again after a recovery and recovering again after a delete both work, whatever `updated_at` says.
- An item is recoverable while it is deleted, not purged, and younger than 30 days since `deleted_at`. The window is a constant, not a setting.
- Recovering a game or a slot only writes its metadata. It never writes live saves: the next checkpoint records whatever is on disk. Recovering a game leaves its separately deleted slots deleted, and a slot is recovered only into a live game.
- A game is not recovered while another live game tracks an overlapping folder or has its title. A recovered slot whose name is taken gets a suffix.
- **Purge** ("Delete now") ends the window early and can't be undone. A purge wins over a concurrent recovery.
- Pruning skips deleted items, so their history stays as it was. Garbage collection counts recoverable games and slots as live, locally and on the remote; their chunks and screenshots are collected only after the window or a purge. The decision uses only shared metadata and the clock; skew near day 30 is covered by the 72 h condemn grace. Retirement markers keep their meaning: a version retired before the delete stays retired after a recovery.
- The metadata of an expired or purged item stays in every store, as before, so devices that were offline don't resurrect it. Sync no longer copies the versions of such items.
- The store format becomes 4. A format 3 store is upgraded in place (the fields are additive, so only the marker changes). A newer format is refused with an error the GUI shows as "update Worldline on this device"; it is never moved aside.

## Considered options

- **Keep the format at 3.** Rejected: an older app on the same remote would condemn and sweep chunks still inside the window, and its merge would drop `recovered_at` whenever it rewrote the metadata. Format 4 makes every device update before it can open the remote again, which is acceptable before 1.0.
- **Last-writer-wins on `updated_at` for the delete flag.** Rejected: an unrelated edit (a rename, an icon) on an offline device would undo a delete or a recovery.
- **A per-user retention setting for the window.** Rejected: one more setting for a rare action; 30 days matches what users expect from a recycle bin.
- **Recovering a game also recovers its deleted slots.** Rejected: a slot deleted on purpose before the game would come back unasked.

## Consequences

- An older app refuses the remote ("unsupported store format") until it is updated. An older app opening a format 4 local repository moves it aside on start, as it does with any format it doesn't know; nothing is deleted.
- Deleted items keep using space for 30 days. "Delete now" frees it sooner.
- A device that recovers a game indexes it again and points it at the slot with the newest version. Other devices pick up the recovery on their next sync; there the game is tracked again unless its folders or title now belong to another game, in which case it stays in the cloud list until adopted.
