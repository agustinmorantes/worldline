---
status: accepted
date: 2026-10-07
---

# Untracking a game is device-local; deleting it is a tombstone

`delete_game` writes `deleted_at` into the game's meta, and sync spreads that tombstone to every device. The GUI offered it as "Remove from library", which in player words means "from here". A single menu item could therefore wipe a game's history on every device.

There are now two operations:

- **Untrack** (UI: "Remove from this PC"): this device stops tracking the game and drops it from its index. The history stays in the store and on the remote. The game shows up again among the remote games not on this device, and can be adopted again. Untracking is the inverse of adoption. It needs a device-local record so automatic adoption of installed remote games does not bring the game straight back.
- **Delete** (UI: "Delete everywhere"): the existing tombstone. The GUI requires a hold to confirm. _Amended by ADR 0011: a deleted game stays recoverable for 30 days, and the hold moves to "Delete now"._

## Considered options

- **Keep only delete, renamed and behind a confirmation.** Rejected: the common intent, "I don't play this on this PC any more", would still have no safe action.
