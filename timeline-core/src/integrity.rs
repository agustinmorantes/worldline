//! Store integrity check: every manifest parses, every chunk a live version
//! needs exists and (deep mode) decompresses to its hash; orphans are counted.

use crate::chunks;
use crate::store::Store;
use anyhow::Result;
use serde::Serialize;
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
    /// Stored chunks no version references (reclaimable by GC).
    pub orphan_chunks: usize,
    pub unreadable_metadata: Option<PathBuf>,
}

impl IntegrityReport {
    pub fn is_ok(&self) -> bool {
        self.missing_chunks.is_empty()
            && self.corrupt_chunks.is_empty()
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
    for game in store.catalog()? {
        for version in game.versions() {
            if retired.contains_key(&version.uuid) {
                continue;
            }
            report.versions_checked += 1;
            for hash in version.chunk_hashes() {
                if !referenced.insert(hash.to_string()) {
                    continue;
                }
                if !stored.contains_key(hash) {
                    report.missing_chunks.push((version.uuid, hash.to_string()));
                    continue;
                }
                report.chunks_checked += 1;
                if deep && chunks::read_chunk(store, hash).is_err() {
                    corrupt.insert(hash.to_string());
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
