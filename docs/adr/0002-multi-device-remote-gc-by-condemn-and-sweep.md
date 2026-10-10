---
status: accepted
date: 2026-09-27
---

# Remote garbage collection with multiple devices: condemn, sweep after a grace period, publishers heal

The remote is a cloud-synced folder. It has no locking and no atomic view, yet pruning must shrink it while several devices are registered.

Garbage collection has two phases:
1. **Condemn.** A device lists the chunks that no non-retired version and no device pointer references, in a write-once `gc/condemned/` record.
2. **Sweep.** A later run deletes only the condemned chunks that are older than a grace period (72 h) and still unreferenced.

Any device may run either phase. Every device that publishes a version re-checks that its chunks exist and re-uploads any that are missing. Pull treats a missing chunk as not yet arrived. Retirement decisions are deterministic, computed from shared settings and the shared catalog, so different devices produce identical write-once markers.

## Considered options

- **Only the sole registered device may delete from the remote** (the earlier design). Rejected: pruning became unusable in the multi-device case, which is the main case.
- **Prune only locally; the remote grows without bound.** Rejected: frequent checkpoints would waste large amounts of remote storage.
- **A lock or lease file on the remote.** Rejected: cloud clients propagate files late and create conflict copies, so a lock gives no real mutual exclusion.
- **A last-seen grace period that drops offline devices.** Rejected: it would silently discard an offline device's progress. Only explicit removal stops protecting a device.

## Consequences

- Chunks can be missing temporarily when a sweep races a delayed publish. They are lost only if every device holding the bytes has lost them too.
- Removal markers are dropped: a removed device that comes back re-registers and is protected again.
