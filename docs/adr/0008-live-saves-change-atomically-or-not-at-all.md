---
status: accepted
date: 2026-10-09
---

# Live saves change atomically or not at all

During release-candidate testing, a rewind with Hollow Knight: Silksong running deleted part of the save folder, hit a file the game held open, and stopped. Nothing was written back, so the game started with no saves. The progress survived only because the pre-restore checkpoint existed.

The rule: **a write to live saves ends in the state before it or the state it was meant to reach, never in between.** If it fails, the user's saves are whole, and the message says nothing was changed.

## Decision

- Every write to live saves goes through `restore::apply`, which runs it as a transaction:
  1. **Stage** the target files into `<data_dir>/txn/<id>/new/<n>`. No live file is touched.
  2. **Move aside** every live, non-excluded entry into `old/<n>`: a rename on the same volume, a copy, fsync, then delete across volumes. A file the game holds fails the rename or the delete, which makes this step the lock check (`SavesInUse`).
  3. **Swap in** the staged files the same way.
  4. **Commit**: delete the transaction folder.
  Any failure before the commit rolls back: placed files are removed and `old/` is moved back. A rollback that can't finish (a file is still held) fails with `RestoreIncomplete` and stays journaled.
- **A failure never asks the user to repair anything.** The core finishes an incomplete rollback by itself: `recover` runs at startup and before every capture (watcher, manual, pre-write), so the watcher's back-off retries it until the files are free. Until then the game isn't captured, so a half-written folder is never recorded or synced, and new writes to it are refused with "try again in a moment". If the game wrote a file where one of ours was moved aside, the game's file stays and ours goes to `<data_dir>/txn/kept/<id>/`, so nothing is lost and nothing stays blocked.
- A journal, `<data_dir>/txn/<id>.json`, records the phase at each step boundary. `WorldlineCore::open` settles leftover journals under the repository lock before the watcher starts, always by **rolling back**: the pre-write checkpoint exists, and the user asked for nothing else.
- **Nothing of ours goes into a game's folders.** The transaction lives in the data dir, so capture, the watcher and the game never see it.
- **Never get in the game's way.** No lock probing and no exclusive opens of a game's files; reads use the default share mode.
- `guarded` refuses (`GameRunning`) when the game is seen running, before checkpointing. When that's unknown, the transaction is the safety net.
- Sync never auto-applies a tip that would empty a non-empty save folder; it stays a divergence for the user.
- Capture rereads a file whose size or modified time changed during the read, and fails the checkpoint if it keeps changing.

## Considered options

- **Temporary folder inside the save folder.** Renames would always stay on one volume, but the game and the watcher would see our folder. Rejected.
- **Check for locks before deleting.** Racy, and an exclusive open can make the game's own save fail. Rejected: the move-aside step is the check.
- **Roll forward after a crash.** The target is known, but the user didn't see it finish, and the game may have started since. Rejected: rolling back is always safe.

## Consequences

- Across volumes the move steps are copies, so a write takes longer; it stays recoverable at every point.
- Disk use peaks at one old and one new copy of the save folders in the data dir.
- A running game that isn't detected can still be upset by its files changing under it; the transaction only guarantees the files end up whole.
