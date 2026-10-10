---
status: accepted
date: 2026-09-27
---

# Space efficiency through content-defined chunking and zstd, not delta chains

The project's requirements ask for "delta compression" and for history to take less than 50% of the space that storing every version as a full copy would.

The mechanism:
- **Whole-file dedup:** an unchanged file is never stored twice.
- **Content-defined chunking** (FastCDC) for files above about 256 KiB, so a partially modified large file shares its unchanged chunks.
- **zstd compression** of every stored chunk.
- **Key:** each chunk is keyed by the SHA-256 of its raw bytes.

This meets the intent of the requirement (store only what changed, compactly) without delta chains.

## Considered options

- **Whole-file dedup only** (the earlier design). Rejected: when a game rewrites a single save file, every version is a new full copy, a ratio of about 1.0.
- **zstd compression only.** Rejected: unchanged regions of large files would still be stored again.
- **Delta chains** (each version stored as a diff against its parent, e.g. zstd with the parent as dictionary). Rejected:
  - restoring must replay a chain of diffs;
  - GC and pruning must keep base versions alive;
  - sync must transfer bases first;
  - a corrupt base breaks every descendant.

  With chunking, every chunk is independently readable, and dedup falls out of content addressing.

## Consequences

- "Delta compression" is read as chunk-level dedup plus compression.
- Hand recovery means decompressing a file's listed chunks with `zstd -d` and concatenating them. [storage-format.md](../storage-format.md) documents this, and the CLI's `repo extract` does it for the user.
- The storage-ratio test must pass three fixtures: a single file with small edits, many files with one changed, and a single file rewritten entirely. The last one is where compression alone has to carry the ratio.
