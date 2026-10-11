//! Content-defined chunking plus zstd (ADR 0003). A file is read once: each
//! chunk is hashed and, when the store lacks it, compressed and published.

use crate::error::ErrorKind;
use crate::store::{FileEntry, Store, format_hash, rename_no_replace, sync_dir};
use anyhow::{Context, Result, anyhow, bail};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Files below this size are stored as a single chunk.
const SINGLE_CHUNK_LIMIT: u64 = 256 * 1024;
const MIN_CHUNK: u32 = 64 * 1024;
const AVG_CHUNK: u32 = 128 * 1024;
const MAX_CHUNK: u32 = 512 * 1024;
const ZSTD_LEVEL: i32 = 3;

/// A tracked directory to capture: its identity, live root and exclusions.
pub struct CaptureRoot<'a> {
    pub uuid: Uuid,
    pub path: &'a Path,
    pub exclusions: &'a [glob::Pattern],
}

/// The most a capture reads. Past it, the capture fails with `TooLarge`
/// before reading any file: a folder that big is rarely only a game's saves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limit {
    pub bytes: u64,
    pub files: usize,
}

/// Captures every regular file below `roots` into `store` and returns the
/// canonical manifest (sorted by tracked dir, then path).
pub fn capture(
    store: &Store,
    roots: &[CaptureRoot],
    limit: Option<Limit>,
) -> Result<Vec<FileEntry>> {
    manifest(Some(store), roots, limit)
}

/// The manifest `capture` would return, without chunk hashes and without
/// writing anything. A file that keeps changing while it is read is a
/// `KeptChanging` error.
pub fn hash(roots: &[CaptureRoot], limit: Option<Limit>) -> Result<Vec<FileEntry>> {
    manifest(None, roots, limit)
}

/// What a capture has listed so far, against its limit.
struct Tally {
    limit: Option<Limit>,
    bytes: u64,
    files: usize,
}

impl Tally {
    fn add(&mut self, size: u64) -> Result<()> {
        self.bytes += size;
        self.files += 1;
        match self.limit {
            Some(limit) if self.bytes > limit.bytes || self.files > limit.files => {
                Err(anyhow::Error::new(ErrorKind::TooLarge).context(format!(
                    "the save folders hold more than {} files or {} MB",
                    limit.files,
                    limit.bytes / 1_000_000
                )))
            }
            _ => Ok(()),
        }
    }
}

/// A file the game kept writing while it was read.
#[derive(Debug)]
pub struct KeptChanging(PathBuf);

impl std::fmt::Display for KeptChanging {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} kept changing while it was read", self.0.display())
    }
}

impl std::error::Error for KeptChanging {}

/// `store`: where the chunks go; `None` only hashes the files.
fn manifest(
    store: Option<&Store>,
    roots: &[CaptureRoot],
    limit: Option<Limit>,
) -> Result<Vec<FileEntry>> {
    // Every root is listed before any file is read, so the limit spares them all.
    let mut tally = Tally {
        limit,
        bytes: 0,
        files: 0,
    };
    let mut listed = Vec::new();
    for root in roots {
        let mut files = Vec::new();
        collect_files(
            root.path,
            root.exclusions,
            &mut Vec::new(),
            true,
            &mut files,
            &mut tally,
        )?;
        listed.push((root.uuid, files));
    }
    let mut entries = Vec::new();
    for (uuid, files) in listed {
        for (relative, path) in files {
            entries.push(capture_stable(store, uuid, relative, &path)?);
        }
    }
    entries.sort();
    Ok(entries)
}

/// Captures `source`, reading it again when its size or modified time changed
/// during the read (the game was writing it), so a torn file is never recorded.
fn capture_stable(
    store: Option<&Store>,
    tracked_dir_uuid: Uuid,
    path: String,
    source: &Path,
) -> Result<FileEntry> {
    for _ in 0..3 {
        let before = stamp(source)?;
        let entry = capture_file(store, tracked_dir_uuid, path.clone(), source)?;
        #[cfg(test)]
        tests::after_read(source);
        if stamp(source)? == before {
            return Ok(entry);
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    Err(KeptChanging(source.to_path_buf()).into())
}

fn stamp(path: &Path) -> Result<(u64, Option<std::time::SystemTime>)> {
    let metadata =
        fs::metadata(path).with_context(|| format!("failed to inspect {}", path.display()))?;
    Ok((metadata.len(), metadata.modified().ok()))
}

fn capture_file(
    store: Option<&Store>,
    tracked_dir_uuid: Uuid,
    path: String,
    source: &Path,
) -> Result<FileEntry> {
    let file =
        File::open(source).with_context(|| format!("failed to open {}", source.display()))?;
    let size = file.metadata()?.len();
    let mut whole = Sha256::new();
    let mut chunks = Vec::new();
    let mut total = 0_u64;
    let mut store_chunk = |data: &[u8]| -> Result<()> {
        whole.update(data);
        total += data.len() as u64;
        if let Some(store) = store {
            let hash = format_hash(&Sha256::digest(data));
            put_chunk(store, &hash, data)?;
            chunks.push(hash);
        }
        Ok(())
    };
    if size < SINGLE_CHUNK_LIMIT {
        let mut data = Vec::with_capacity(size as usize);
        BufReader::new(file).read_to_end(&mut data)?;
        if !data.is_empty() {
            store_chunk(&data)?;
        }
    } else {
        let chunker =
            fastcdc::v2020::StreamCDC::new(BufReader::new(file), MIN_CHUNK, AVG_CHUNK, MAX_CHUNK);
        for chunk in chunker {
            let chunk =
                chunk.map_err(|error| anyhow!("failed to read {}: {error}", source.display()))?;
            store_chunk(&chunk.data)?;
        }
    }
    Ok(FileEntry {
        tracked_dir_uuid,
        path,
        size: total,
        sha256: format_hash(&whole.finalize()),
        chunks,
    })
}

/// Publishes one chunk unless the store already has it. Existing chunks are
/// trusted by existence: verification happens when they are read.
pub fn put_chunk(store: &Store, hash: &str, data: &[u8]) -> Result<()> {
    if store.has_chunk(hash) {
        return Ok(());
    }
    let compressed = zstd::bulk::compress(data, ZSTD_LEVEL)?;
    publish_chunk(store, hash, &compressed)
}

/// Writes already-compressed chunk bytes durably: temp file, fsync, rename
/// without replacing, directory sync. Used by capture and by transfer.
pub fn publish_chunk(store: &Store, hash: &str, compressed: &[u8]) -> Result<()> {
    let destination = store.chunk_path(hash)?;
    let shard = destination.parent().context("chunk path has no parent")?;
    fs::create_dir_all(shard).with_context(|| format!("failed to create {}", shard.display()))?;
    let temp = shard.join(format!(".tmp-{}", crate::store::short_random()));
    let result = (|| -> Result<()> {
        let mut file = File::options()
            .write(true)
            .create_new(true)
            .open(&temp)
            .with_context(|| format!("failed to create {}", temp.display()))?;
        file.write_all(compressed)?;
        file.sync_all()?;
        drop(file);
        match rename_no_replace(&temp, &destination) {
            Ok(()) => sync_dir(shard),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let _ = fs::remove_file(&temp);
            }
            Err(error) => return Err(error.into()),
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result.with_context(|| format!("failed to publish chunk {hash}"))
}

/// Reads, decompresses and verifies one chunk. A chunk that is present but
/// fails verification is moved aside so the next capture republishes it. Only
/// for the local repository: on a sync remote a failing chunk is "not yet
/// arrived", not damage.
pub fn read_chunk(store: &Store, hash: &str) -> Result<Vec<u8>> {
    let compressed = read_compressed(store, hash)?;
    decode_verified(hash, &compressed).inspect_err(|_| {
        let _ = store.quarantine_chunk(hash);
    })
}

fn decode_verified(hash: &str, compressed: &[u8]) -> Result<Vec<u8>> {
    let data = zstd::stream::decode_all(compressed)
        .with_context(|| format!("chunk {hash} is not a valid zstd frame"))?;
    if format_hash(&Sha256::digest(&data)) != hash {
        bail!("chunk {hash} failed verification");
    }
    Ok(data)
}

/// The stored (compressed) bytes, refusing links and non-files.
pub fn read_compressed(store: &Store, hash: &str) -> Result<Vec<u8>> {
    let path = store.chunk_path(hash)?;
    let metadata =
        fs::symlink_metadata(&path).with_context(|| format!("chunk {hash} is missing"))?;
    if !metadata.is_file() || crate::fs_utils::is_link_or_reparse(&metadata) {
        bail!("chunk {hash} is not a regular file");
    }
    fs::read(&path).with_context(|| format!("failed to read chunk {hash}"))
}

/// Rebuilds a file from its chunks into `out`, verifying every chunk and the
/// whole-file hash.
pub fn write_file(store: &Store, entry: &FileEntry, out: &mut impl Write) -> Result<()> {
    let mut whole = Sha256::new();
    let mut size = 0;
    for hash in &entry.chunks {
        let data = read_chunk(store, hash)?;
        whole.update(&data);
        size += data.len() as u64;
        out.write_all(&data)?;
    }
    if size != entry.size || format_hash(&whole.finalize()) != entry.sha256 {
        bail!("{} does not match its manifest", entry.path);
    }
    Ok(())
}

/// Every chunk of `entry` is present and valid (without writing anything).
pub fn verify_file(store: &Store, entry: &FileEntry) -> Result<()> {
    write_file(store, entry, &mut io::sink())
}

fn collect_files(
    dir: &Path,
    exclusions: &[glob::Pattern],
    relative: &mut Vec<String>,
    is_root: bool,
    files: &mut Vec<(String, PathBuf)>,
    tally: &mut Tally,
) -> Result<()> {
    let read = match fs::read_dir(dir) {
        Ok(read) => read,
        Err(error) if is_root && error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", dir.display()));
        }
    };
    for entry in read {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().into_string().map_err(|name| {
            anyhow!(
                "cannot capture {}: file names must be valid Unicode ({name:?})",
                path.display()
            )
        })?;
        if name.contains('\\') {
            bail!(
                "cannot capture {}: file names must not contain '\\'",
                path.display()
            );
        }
        if crate::fs_utils::is_excluded_file(&name) {
            continue;
        }
        relative.push(name);
        let relative_path = relative.join("/");
        if !crate::exclusions::is_excluded(exclusions, Path::new(&relative_path)) {
            let metadata = fs::symlink_metadata(&path)
                .with_context(|| format!("failed to inspect {}", path.display()))?;
            if metadata.is_dir() {
                collect_files(&path, exclusions, relative, false, files, tally)?;
            } else if metadata.is_file() {
                tally.add(metadata.len())?;
                files.push((relative_path, path));
            }
        }
        relative.pop();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use tempfile::tempdir;

    thread_local! {
        /// How many more reads the file changes after.
        static CHANGES_AFTER_READ: Cell<usize> = const { Cell::new(0) };
    }

    pub(super) fn after_read(source: &Path) {
        let left = CHANGES_AFTER_READ.get();
        if left > 0 {
            CHANGES_AFTER_READ.set(left - 1);
            let mut contents = fs::read(source).unwrap();
            contents.push(b'+');
            fs::write(source, contents).unwrap();
        }
    }

    #[test]
    fn a_file_written_during_the_read_is_read_again() {
        let store_dir = tempdir().unwrap();
        let store = Store::open(store_dir.path()).unwrap();
        let live = tempdir().unwrap();
        fs::write(live.path().join("save.dat"), b"v").unwrap();

        CHANGES_AFTER_READ.set(1);
        let manifest = capture(&store, &roots(live.path()), None).unwrap();
        assert_eq!(manifest[0].size, 2, "the settled file was recorded");

        CHANGES_AFTER_READ.set(3);
        let error = capture(&store, &roots(live.path()), None).unwrap_err();
        CHANGES_AFTER_READ.set(0);
        assert!(format!("{error:#}").contains("kept changing"));
    }

    #[test]
    fn a_capture_over_its_limit_fails_before_reading() {
        let store_dir = tempdir().unwrap();
        let store = Store::open(store_dir.path()).unwrap();
        let live = tempdir().unwrap();
        fs::write(live.path().join("a.dat"), b"12345").unwrap();
        fs::write(live.path().join("b.dat"), b"12345").unwrap();

        let fits = Limit {
            bytes: 10,
            files: 2,
        };
        assert_eq!(
            capture(&store, &roots(live.path()), Some(fits))
                .unwrap()
                .len(),
            2
        );
        for over in [
            Limit { bytes: 9, files: 2 },
            Limit {
                bytes: 10,
                files: 1,
            },
        ] {
            let error = hash(&roots(live.path()), Some(over)).unwrap_err();
            assert_eq!(crate::error::error_kind(&error), Some(ErrorKind::TooLarge));
        }
    }

    fn roots(path: &Path) -> Vec<CaptureRoot<'_>> {
        vec![CaptureRoot {
            uuid: Uuid::from_u128(1),
            path,
            exclusions: &[],
        }]
    }

    #[test]
    fn capture_round_trips_and_deduplicates() {
        let store_dir = tempdir().unwrap();
        let store = Store::open(store_dir.path()).unwrap();
        let live = tempdir().unwrap();
        fs::create_dir_all(live.path().join("sub")).unwrap();
        fs::write(live.path().join("a.dat"), b"same").unwrap();
        fs::write(live.path().join("sub").join("b.dat"), b"same").unwrap();
        fs::write(live.path().join("empty.dat"), b"").unwrap();
        fs::write(live.path().join("ignored.log"), b"x").unwrap();
        let big: Vec<u8> = (0..3_000_000u32)
            .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
            .collect();
        fs::write(live.path().join("big.sav"), &big).unwrap();

        let manifest = capture(&store, &roots(live.path()), None).unwrap();
        let paths: Vec<_> = manifest.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["a.dat", "big.sav", "empty.dat", "sub/b.dat"]);
        assert_eq!(
            manifest[0].chunks, manifest[3].chunks,
            "identical files share chunks"
        );
        assert!(manifest[1].chunks.len() > 1, "large files are chunked");
        assert!(manifest[2].chunks.is_empty());

        let mut rebuilt = Vec::new();
        write_file(&store, &manifest[1], &mut rebuilt).unwrap();
        assert_eq!(rebuilt, big);
        assert_eq!(
            capture(&store, &roots(live.path()), None).unwrap(),
            manifest
        );
    }

    #[test]
    fn corrupt_chunk_fails_verification_but_never_blocks_capture() {
        let store_dir = tempdir().unwrap();
        let store = Store::open(store_dir.path()).unwrap();
        let live = tempdir().unwrap();
        fs::write(live.path().join("save.dat"), b"progress").unwrap();
        let manifest = capture(&store, &roots(live.path()), None).unwrap();
        let path = store.chunk_path(&manifest[0].chunks[0]).unwrap();
        fs::write(&path, b"garbage").unwrap();
        assert!(verify_file(&store, &manifest[0]).is_err());
        // The failed read set the bad chunk aside, so the next capture heals it.
        capture(&store, &roots(live.path()), None).unwrap();
        verify_file(&store, &manifest[0]).unwrap();
    }

    #[test]
    fn missing_root_captures_nothing() {
        let store_dir = tempdir().unwrap();
        let store = Store::open(store_dir.path()).unwrap();
        let missing = store_dir.path().join("nope");
        assert!(capture(&store, &roots(&missing), None).unwrap().is_empty());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn rejects_non_unicode_names() {
        use std::os::unix::ffi::OsStrExt;
        let store_dir = tempdir().unwrap();
        let store = Store::open(store_dir.path()).unwrap();
        let live = tempdir().unwrap();
        fs::write(
            live.path().join(std::ffi::OsStr::from_bytes(b"bad\xff")),
            b"x",
        )
        .unwrap();
        assert!(capture(&store, &roots(live.path()), None).is_err());
    }
}
