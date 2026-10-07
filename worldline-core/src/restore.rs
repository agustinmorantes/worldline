//! Writing a version's content back into the live tracked directories.
//! Everything is validated (paths, and every chunk decompressed and hashed)
//! before the first live file is touched.

use crate::chunks;
use crate::models::TrackedDir;
use crate::store::{FileEntry, GameRecord, Store, VersionMeta};
use anyhow::{Context, Result, bail};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub struct PreparedRestore {
    /// Live roots to clear (except their exclusions) before writing.
    roots: BTreeMap<PathBuf, Vec<glob::Pattern>>,
    files: Vec<(PathBuf, FileEntry)>,
}

/// Prepares restoring `version`. `record` supplies every tracked directory the
/// game ever had (retired ones too); `live_dirs` are the current ones, which
/// are cleared even when the version holds nothing for them.
pub fn prepare(
    store: &Store,
    record: &GameRecord,
    live_dirs: &[TrackedDir],
    version: &VersionMeta,
    retired: bool,
) -> Result<PreparedRestore> {
    if retired {
        bail!("This checkpoint was retired and can no longer be restored");
    }
    let mut known: HashMap<Uuid, (PathBuf, Vec<glob::Pattern>)> = HashMap::new();
    for dir in &record.tracked_dirs {
        let path = crate::path_placeholders::expand_path_placeholders(&dir.path);
        let exclusions = dir
            .exclusions
            .iter()
            .filter_map(|pattern| glob::Pattern::new(pattern).ok())
            .collect();
        known.insert(dir.uuid, (PathBuf::from(path), exclusions));
    }
    let mut prepared = clear_only(live_dirs)?;
    for dir in live_dirs {
        let uuid = Uuid::parse_str(&dir.uuid)?;
        known.insert(
            uuid,
            (
                PathBuf::from(&dir.path),
                crate::exclusions::parse(&dir.exclusions),
            ),
        );
    }
    for file in &version.files {
        let (root, exclusions) = known.get(&file.tracked_dir_uuid).with_context(|| {
            format!(
                "Version references unknown tracked directory {}",
                file.tracked_dir_uuid
            )
        })?;
        prepared
            .roots
            .entry(root.clone())
            .or_insert_with(|| exclusions.clone());
        let relative = Path::new(&file.path);
        if crate::exclusions::is_excluded(exclusions, relative) {
            continue;
        }
        crate::fs_utils::validate_restore_path(root, relative).with_context(|| {
            format!(
                "Unsafe restore destination {}",
                root.join(relative).display()
            )
        })?;
        chunks::verify_file(store, file)
            .with_context(|| format!("Stored copy of {} is damaged", file.path))?;
        prepared.files.push((root.join(relative), file.clone()));
    }
    for root in prepared.roots.keys() {
        crate::fs_utils::validate_restore_path(root, Path::new(""))
            .with_context(|| format!("Unsafe tracked dir {}", root.display()))?;
    }
    Ok(prepared)
}

/// Prepares emptying the live directories (activating a slot with no versions).
pub fn clear_only(live_dirs: &[TrackedDir]) -> Result<PreparedRestore> {
    let mut roots = BTreeMap::new();
    for dir in live_dirs {
        let path = PathBuf::from(&dir.path);
        crate::fs_utils::validate_restore_path(&path, Path::new(""))
            .with_context(|| format!("Unsafe tracked dir {}", dir.path))?;
        roots.insert(path, crate::exclusions::parse(&dir.exclusions));
    }
    Ok(PreparedRestore {
        roots,
        files: Vec::new(),
    })
}

pub fn apply(store: &Store, prepared: PreparedRestore) -> Result<()> {
    for (root, exclusions) in &prepared.roots {
        if root.exists() {
            crate::fs_utils::clear_dir_except(root, exclusions)
                .with_context(|| format!("Failed to clear tracked dir {}", root.display()))?;
        }
        std::fs::create_dir_all(root)
            .with_context(|| format!("Failed to create tracked dir {}", root.display()))?;
    }
    for (destination, entry) in prepared.files {
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        let mut file = std::fs::File::create(&destination)
            .with_context(|| format!("Failed to create {}", destination.display()))?;
        chunks::write_file(store, &entry, &mut file)
            .with_context(|| format!("Failed to restore {}", destination.display()))?;
        file.sync_all()?;
    }
    Ok(())
}
