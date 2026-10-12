# Glossary

The domain language used in Worldline's code, docs and [decision records](adr/). Worldline tracks game save directories as a non-destructive, branching history that can be restored, synced between devices, and exported. Conflicting progress is kept as coexisting branches, never overwritten.

## Language

### History

**Game**:
A tracked title whose save data is versioned.
_Avoid_: title, app

**Tracked directory**:
A save directory on disk that belongs to a game and is captured as a whole, minus its exclusions.
_Avoid_: save path, watched folder

**Slot**:
An independent history line of a game, for a separate playthrough. Exactly one slot per game is active on a device.
_Avoid_: profile, save slot (in-game), branch

**Version**:
An immutable capture of every tracked directory of a game at one moment, identified by UUID. Its number is only a display hint.
_Avoid_: snapshot, save, checkpoint (as a noun)

**Checkpoint**:
The act of capturing live saves into a new version; it produces nothing when the content is unchanged.
_Avoid_: backup, snapshot

**Restore**:
Writing a version's content back into the live tracked directories.
_Avoid_: rollback, load

**Branch**:
A line of versions diverging from a shared parent inside one slot, created by restoring an older version and playing on, or by two devices playing offline.
_Avoid_: fork (for the line; a fork is the version where lines split), conflict

**Tag**:
A user-given label on a version that also protects it from pruning.
_Avoid_: bookmark, name

**Play session**:
A derived grouping of consecutive versions from one device with no gap above the session threshold. Computed for display, never stored.
_Avoid_: session (alone, ambiguous with app sessions)

### Storage

**Store**:
A directory holding history in the shared on-disk format. The local repository, the remote and a bundle are all stores.
_Avoid_: repo (ambiguous with git), archive

**Chunk**:
A content-addressed, compressed piece of a file, stored once and shared by every version that contains it.
_Avoid_: blob (legacy name), object

**Manifest**:
A version's list of files and the chunks that rebuild each one.
_Avoid_: file list, index

**Index**:
The rebuildable, device-local database that speeds up queries over a store. It is never the source of truth.
_Avoid_: database (as authority), cache

### Devices and sync

**Device**:
One installation of the app, with a stable identity, that publishes its own pointers to the remote.
_Avoid_: machine, client

**Device kind**:
What a device is (desktop, laptop or handheld), chosen by the user and published with the device. Never detected from the OS.

**Remote**:
The shared store in a cloud-synced folder, through which devices exchange history.
_Avoid_: cloud, server

**Game stamp**:
What a device has published of a game, used by other devices to notice changes and identify data still arriving.

**Active version**:
The version a device's live saves currently correspond to, per slot. It is a per-device fact, never shared history.
_Avoid_: current version, HEAD

**Fast-forward**:
When another device's active version descends from this device's, so applying it loses nothing.

**Divergence**:
When two devices' active versions share an ancestor but neither descends from the other; both are kept as branches.
_Avoid_: conflict

**Adoption**:
Starting to track, on this device, a game that exists on the remote.
_Avoid_: download, import (import is for bundles)

**Untrack**:
Stopping tracking of a game on this device while its history stays on the remote; the inverse of adoption. Deleting a game, by contrast, writes a tombstone that reaches every device.
_Avoid_: remove (ambiguous with delete)

**Bundle**:
A standalone store containing one game's history, used for export and import.
_Avoid_: backup, archive, zip

### Retention

**Pruning**:
Automatic reduction of stored history according to the user's retention settings.
_Avoid_: cleanup, deletion

**Retirement**:
Marking a version as pruned. Its graph node stays (a tombstone) but its content is no longer restorable.
_Avoid_: deleting a version

**Tombstone**:
A retired version, or a deleted slot or game, kept only as a record so other devices don't resurrect it.

**Garbage collection**:
Removal of chunks that no live version or device pointer references.
_Avoid_: purge

## UI vocabulary

UI copy uses player words; code, docs and this glossary keep the domain terms above. The "_Avoid_" lists apply to code and docs, not to UI copy.

| UI says | Domain term |
|---|---|
| checkpoint (noun) | version |
| name a checkpoint | tag |
| playthrough | slot |
| timeline, another timeline | branch |
| rewind, switch | restore |
| cloud | remote |
| session | play session |
| remove from this PC | untrack |
| delete everywhere | delete (tombstone) |
