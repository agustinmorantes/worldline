//! Store integrity check: every manifest parses, every chunk a live version
//! needs exists and (deep mode) decompresses to its hash and rebuilds each
//! file with the size and hash its manifest records; orphans are counted.

use crate::chunks;
use crate::store::{FileEntry, Store, format_hash};
use anyhow::Result;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Default, Clone, Serialize, PartialEq, Eq)]
pub struct IntegrityReport {
    pub versions_checked: usize,
    pub chunks_checked: usize,
    /// (version, chunk) pairs whose chunk is absent.
    pub missing_chunks: Vec<(Uuid, String)>,
    /// Chunks that exist but fail decompression or their hash (deep mode).
    pub corrupt_chunks: Vec<String>,
    /// (version, file path) pairs whose valid chunks don't rebuild the file
    /// the manifest describes (deep mode).
    pub damaged_files: Vec<(Uuid, String)>,
    /// Stored chunks no version references (reclaimable by GC).
    pub orphan_chunks: usize,
    pub unreadable_metadata: Option<PathBuf>,
}

impl IntegrityReport {
    pub fn is_ok(&self) -> bool {
        self.missing_chunks.is_empty()
            && self.corrupt_chunks.is_empty()
            && self.damaged_files.is_empty()
            && self.unreadable_metadata.is_none()
    }
}

pub fn check(store: &Store, deep: bool) -> Result<IntegrityReport> {
    let mut report = IntegrityReport {
        unreadable_metadata: store.first_unloadable_metadata()?,
        ..IntegrityReport::default()
    };
    let retired = store.markers()?;
    let stored = store.chunk_sizes()?;
    let mut referenced: HashSet<String> = HashSet::new();
    let mut corrupt = BTreeSet::new();
    // Unchanged files repeat across versions; each distinct entry is rebuilt once.
    let mut rebuilt: HashSet<&FileEntry> = HashSet::new();
    let catalog = store.catalog()?;
    for game in &catalog {
        for version in game.versions() {
            if retired.contains_key(&version.uuid) {
                continue;
            }
            report.versions_checked += 1;
            for file in &version.files {
                let mut complete = true;
                for hash in &file.chunks {
                    if !stored.contains_key(hash) {
                        complete = false;
                    }
                    if !referenced.insert(hash.to_string()) {
                        continue;
                    }
                    if stored.contains_key(hash) {
                        report.chunks_checked += 1;
                    } else {
                        report.missing_chunks.push((version.uuid, hash.to_string()));
                    }
                }
                if deep && complete && rebuilt.insert(file) && !rebuilds(store, file, &mut corrupt)
                {
                    report.damaged_files.push((version.uuid, file.path.clone()));
                }
            }
        }
    }
    report.corrupt_chunks = corrupt.into_iter().collect();
    report.orphan_chunks = stored
        .keys()
        .filter(|hash| !referenced.contains(*hash))
        .count();
    Ok(report)
}

/// Whether `file`'s chunks rebuild it. A corrupt chunk goes to `corrupt`
/// instead and doesn't count against the file.
fn rebuilds(store: &Store, file: &FileEntry, corrupt: &mut BTreeSet<String>) -> bool {
    let mut whole = Sha256::new();
    let mut size = 0;
    for hash in &file.chunks {
        if corrupt.contains(hash) {
            return true;
        }
        match chunks::read_chunk(store, hash) {
            Ok(data) => {
                whole.update(&data);
                size += data.len() as u64;
            }
            Err(_) => {
                corrupt.insert(hash.clone());
                return true;
            }
        }
    }
    size == file.size && format_hash(&whole.finalize()) == file.sha256
}
