//! Read-only retention selection. Deletion needs the durable retirement protocol.

use crate::repository::{self, VersionMetadata};
use crate::sync::{DeviceState, RemoteCatalog};
use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Datelike, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Bucket {
    Hour(i32, u32, u32, u32),
    Day(i32, u32, u32),
    Week(i32, u32),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetirementMarker {
    pub version_uuid: Uuid,
    pub slot_uuid: Uuid,
    pub content_hash: String,
    pub retired_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlobGcPreview {
    pub protected_blobs: usize,
    pub reclaimable_blobs: usize,
    pub reclaimable_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PruningPreview {
    pub retire_versions: Vec<Uuid>,
    pub local_gc: BlobGcPreview,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PruningRunReport {
    pub retired_versions: Vec<Uuid>,
    pub local_gc: BlobGcPreview,
    pub remote_gc: Option<BlobGcPreview>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PruningSettings {
    pub automatic: bool,
}

pub fn load_settings(path: &Path) -> Result<PruningSettings> {
    match fs::symlink_metadata(path) {
        Ok(_) => repository::read_json_file(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(PruningSettings::default())
        }
        Err(error) => Err(error.into()),
    }
}

pub fn save_settings(path: &Path, settings: PruningSettings) -> Result<()> {
    repository::write_json_file(path, &settings)
}

pub fn marker_path(root: &Path, version_uuid: Uuid) -> PathBuf {
    root.join("retired-versions")
        .join(format!("{version_uuid}.json"))
}

pub fn write_marker(root: &Path, marker: &RetirementMarker) -> Result<()> {
    let path = marker_path(root, marker.version_uuid);
    let parent = path.parent().context("Retirement marker has no parent")?;
    fs::create_dir_all(parent)?;
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if !metadata.is_file() || crate::fs_utils::is_link_or_reparse(&metadata) {
                return Err(anyhow!("Retirement marker is not a regular file"));
            }
            let existing: RetirementMarker = repository::read_json_file(&path)?;
            return if existing.version_uuid == marker.version_uuid
                && existing.slot_uuid == marker.slot_uuid
                && existing.content_hash == marker.content_hash
            {
                Ok(())
            } else {
                Err(anyhow!("Conflicting retirement marker"))
            };
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let staging = parent.join(format!(".retirement-{}.tmp", Uuid::new_v4()));
    repository::write_json_file(&staging, marker)?;
    match crate::content_store::atomic_rename_no_replace(&staging, &path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&staging);
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                write_marker(root, marker)
            } else {
                Err(error).context("Failed to publish retirement marker")
            }
        }
    }
}

pub fn read_markers(root: &Path) -> Result<HashMap<Uuid, RetirementMarker>> {
    let dir = root.join("retired-versions");
    let metadata = match fs::symlink_metadata(&dir) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_dir() || crate::fs_utils::is_link_or_reparse(&metadata) {
        return Err(anyhow!("Retirement directory is not a regular directory"));
    }
    let mut markers = HashMap::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_str().context("Non-UTF8 retirement marker name")?;
        if name.starts_with('.') {
            continue; // Incomplete staging files are not published markers.
        }
        let uuid = name
            .strip_suffix(".json")
            .and_then(|name| Uuid::parse_str(name).ok())
            .context("Invalid retirement marker filename")?;
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.is_file() || crate::fs_utils::is_link_or_reparse(&metadata) {
            return Err(anyhow!("Retirement marker is not a regular file"));
        }
        let marker: RetirementMarker = repository::read_json_file(&path)?;
        if marker.version_uuid != uuid || markers.insert(uuid, marker).is_some() {
            return Err(anyhow!("Retirement marker identity mismatch"));
        }
    }
    Ok(markers)
}

pub fn validate_markers(catalog: &RemoteCatalog) -> Result<()> {
    let versions = catalog
        .games
        .iter()
        .flat_map(|game| &game.slots)
        .flat_map(|slot| &slot.versions)
        .map(|version| (version.version.uuid, &version.version))
        .collect::<HashMap<_, _>>();
    for marker in catalog.retired_versions.values() {
        let version = versions
            .get(&marker.version_uuid)
            .context("Retirement marker references a missing version")?;
        if version.slot_uuid != Some(marker.slot_uuid)
            || version.content_hash != marker.content_hash
        {
            return Err(anyhow!("Retirement marker does not match version manifest"));
        }
    }
    Ok(())
}

/// Unlike normal sync reads, a maintenance decision must see every published entry.
pub fn validate_catalog_complete(root: &Path) -> Result<()> {
    let removed = crate::sync::removed_devices(root)?;
    for device_dir in child_dirs(&root.join("devices"))? {
        let directory_uuid = device_dir
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| Uuid::parse_str(name).ok())
            .context("Invalid remote device directory")?;
        if removed.contains(&directory_uuid) {
            continue;
        }
        ensure_regular_file(&device_dir.join("device.json"))?;
        ensure_regular_file(&device_dir.join("state.json"))?;
        let device: crate::sync::DeviceMetadata =
            repository::read_json_file(&device_dir.join("device.json"))?;
        let state: DeviceState = repository::read_json_file(&device_dir.join("state.json"))?;
        if device.device_uuid != directory_uuid || state.device_uuid != directory_uuid {
            return Err(anyhow!("Remote device identity mismatch"));
        }
    }
    for game_dir in child_dirs(&repository::games_dir(root))? {
        ensure_regular_file(&repository::metadata_path(&game_dir))?;
        let game: crate::sync::RemoteGameMetadata = repository::read_metadata_json(&game_dir)?;
        for tracked_dir in child_dirs(&repository::tracked_dirs_dir(&game_dir))? {
            ensure_regular_file(&repository::metadata_path(&tracked_dir))?;
            let tracked: repository::TrackedDirMetadata =
                repository::read_metadata_json(&tracked_dir)?;
            if tracked.game_uuid != game.uuid {
                return Err(anyhow!("Remote tracked directory belongs to another game"));
            }
        }
        for slot_dir in child_dirs(&repository::slots_dir(&game_dir))? {
            ensure_regular_file(&repository::metadata_path(&slot_dir))?;
            let slot: crate::sync::RemoteSlotMetadata = repository::read_metadata_json(&slot_dir)?;
            if slot.game_uuid != game.uuid {
                return Err(anyhow!("Remote slot belongs to another game"));
            }
            for version_dir in child_dirs(&repository::versions_dir(&slot_dir))? {
                ensure_regular_file(&repository::metadata_path(&version_dir))?;
                let version: crate::sync::RemoteVersionMetadata =
                    repository::read_metadata_json(&version_dir)?;
                if version.version.game_uuid != game.uuid
                    || version.version.slot_uuid != Some(slot.uuid)
                {
                    return Err(anyhow!("Remote version belongs to another game or slot"));
                }
            }
        }
    }
    Ok(())
}

fn ensure_regular_file(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || crate::fs_utils::is_link_or_reparse(&metadata) {
        return Err(anyhow!("Repository metadata is not a regular file"));
    }
    Ok(())
}

/// Include unpublished local history in a remote preview without changing either repository.
pub fn merge_local_history(catalog: &mut RemoteCatalog, local: RemoteCatalog) -> Result<()> {
    for (uuid, marker) in local.retired_versions {
        if let Some(existing) = catalog.retired_versions.insert(uuid, marker.clone())
            && (existing.slot_uuid != marker.slot_uuid
                || existing.content_hash != marker.content_hash)
        {
            return Err(anyhow!("Local and remote retirement markers conflict"));
        }
    }
    for game in local.games {
        let Some(remote_game) = catalog
            .games
            .iter_mut()
            .find(|remote| remote.metadata.uuid == game.metadata.uuid)
        else {
            catalog.games.push(game);
            continue;
        };
        for slot in game.slots {
            let Some(remote_slot) = remote_game
                .slots
                .iter_mut()
                .find(|remote| remote.metadata.uuid == slot.metadata.uuid)
            else {
                remote_game.slots.push(slot);
                continue;
            };
            for version in slot.versions {
                if let Some(remote_version) = remote_slot
                    .versions
                    .iter_mut()
                    .find(|remote| remote.version.uuid == version.version.uuid)
                {
                    let mut local_metadata = version.version.clone();
                    let mut remote_metadata = remote_version.version.clone();
                    local_metadata.tag = None;
                    remote_metadata.tag = None;
                    if local_metadata != remote_metadata {
                        return Err(anyhow!("Local and remote version metadata disagree"));
                    }
                    if version
                        .version
                        .tag
                        .as_deref()
                        .is_some_and(|tag| !tag.trim().is_empty())
                    {
                        remote_version.version.tag = version.version.tag;
                    }
                } else {
                    remote_slot.versions.push(version);
                }
            }
        }
    }
    validate_markers(catalog)
}

fn local_versions(root: &Path) -> Result<Vec<VersionMetadata>> {
    let mut versions = Vec::new();
    let games = repository::games_dir(root);
    if !games.exists() {
        return Ok(versions);
    }
    for game in child_dirs(&games)? {
        for slot in child_dirs(&repository::slots_dir(&game))? {
            for version in child_dirs(&repository::versions_dir(&slot))? {
                let path = repository::metadata_path(&version);
                if path.exists() {
                    ensure_regular_file(&path)?;
                    versions.push(repository::read_json_file(&path)?);
                } else {
                    return Err(anyhow!("Incomplete local version at {}", version.display()));
                }
            }
        }
    }
    Ok(versions)
}

fn child_dirs(parent: &Path) -> Result<Vec<PathBuf>> {
    if !parent.exists() {
        return Ok(Vec::new());
    }
    let metadata = fs::symlink_metadata(parent)?;
    if !metadata.is_dir() || crate::fs_utils::is_link_or_reparse(&metadata) {
        return Err(anyhow!("Repository entry is not a regular directory"));
    }
    let mut dirs = Vec::new();
    for entry in fs::read_dir(parent)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if !metadata.is_dir() || crate::fs_utils::is_link_or_reparse(&metadata) {
            return Err(anyhow!("Repository contains an unexpected entry"));
        }
        dirs.push(entry.path());
    }
    Ok(dirs)
}

pub fn preview_local_gc(root: &Path, active_versions: &HashSet<Uuid>) -> Result<BlobGcPreview> {
    collect_local_gc(root, active_versions).map(|(preview, _)| preview)
}

pub fn run_local_gc(root: &Path, active_versions: &HashSet<Uuid>) -> Result<BlobGcPreview> {
    repository::with_exclusive_lock(root, || run_local_gc_unlocked(root, active_versions))
}

/// Caller must hold the repository's exclusive lock.
pub(crate) fn run_local_gc_unlocked(
    root: &Path,
    active_versions: &HashSet<Uuid>,
) -> Result<BlobGcPreview> {
    let (preview, paths) = collect_local_gc(root, active_versions)?;
    for path in paths {
        fs::remove_file(&path)
            .with_context(|| format!("Failed to remove unreferenced blob {}", path.display()))?;
    }
    Ok(preview)
}

fn collect_local_gc(
    root: &Path,
    active_versions: &HashSet<Uuid>,
) -> Result<(BlobGcPreview, Vec<PathBuf>)> {
    let markers = read_markers(root)?;
    let versions = local_versions(root)?;
    let mut protected = HashSet::new();
    let mut seen = HashSet::new();
    for version in &versions {
        if !seen.insert(version.uuid) {
            return Err(anyhow!("Duplicate local version UUID"));
        }
        if let Some(marker) = markers.get(&version.uuid)
            && (version.slot_uuid != Some(marker.slot_uuid)
                || version.content_hash != marker.content_hash)
        {
            return Err(anyhow!("Retirement marker does not match version manifest"));
        }
        if !markers.contains_key(&version.uuid) || active_versions.contains(&version.uuid) {
            for file in &version.files {
                crate::content_store::blob_path(root, &file.sha256)?;
                protected.insert(file.sha256.clone());
            }
        }
    }
    if markers.keys().any(|uuid| !seen.contains(uuid))
        || active_versions.iter().any(|uuid| !seen.contains(uuid))
    {
        return Err(anyhow!(
            "Retirement or active pointer references a missing version"
        ));
    }
    let mut reclaimable_blobs = 0;
    let mut reclaimable_bytes = 0;
    let mut reclaimable_paths = Vec::new();
    let blobs = repository::blobs_dir(root);
    for shard in child_dirs(&blobs)? {
        let shard_name = shard
            .file_name()
            .and_then(|s| s.to_str())
            .context("Invalid blob shard")?;
        for entry in fs::read_dir(&shard)? {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_str().context("Invalid blob name")?;
            let hash = format!("sha256:{shard_name}{name}");
            if crate::content_store::blob_path(root, &hash)? != path {
                return Err(anyhow!("Malformed blob path"));
            }
            let metadata = fs::symlink_metadata(&path)?;
            if !metadata.is_file() || crate::fs_utils::is_link_or_reparse(&metadata) {
                return Err(anyhow!("Blob entry is not a regular file"));
            }
            if !protected.contains(&hash) {
                reclaimable_blobs += 1;
                reclaimable_bytes += metadata.len();
                reclaimable_paths.push(path);
            }
        }
    }
    Ok((
        BlobGcPreview {
            protected_blobs: protected.len(),
            reclaimable_blobs,
            reclaimable_bytes,
        },
        reclaimable_paths,
    ))
}

fn bucket(created: DateTime<Utc>, now: DateTime<Utc>) -> Option<Bucket> {
    let age = now.signed_duration_since(created);
    if age < chrono::Duration::hours(1) {
        None
    } else if age < chrono::Duration::hours(24) {
        Some(Bucket::Hour(
            created.year(),
            created.month(),
            created.day(),
            created.hour(),
        ))
    } else if age <= chrono::Duration::days(7) {
        Some(Bucket::Day(created.year(), created.month(), created.day()))
    } else {
        let week = created.iso_week();
        Some(Bucket::Week(week.year(), week.week()))
    }
}

/// Returns version UUIDs whose payload may be retired, sorted for stable previews.
/// No filesystem or database mutations occur here.
pub fn plan_retirement(
    catalog: &RemoteCatalog,
    local_state: &DeviceState,
    now: DateTime<Utc>,
) -> Result<Vec<Uuid>> {
    let device_ids: HashSet<_> = catalog.devices.iter().map(|d| d.device_uuid).collect();
    if device_ids.len() != catalog.devices.len() {
        return Err(anyhow!("Duplicate remote device identity"));
    }
    let state_ids: HashSet<_> = catalog.states.iter().map(|s| s.device_uuid).collect();
    if state_ids.len() != catalog.states.len() || state_ids != device_ids {
        return Err(anyhow!("Remote device state is missing or inconsistent"));
    }

    let mut active = HashMap::new();
    for state in catalog.states.iter().chain(std::iter::once(local_state)) {
        for (game_uuid, game) in &state.games {
            for (slot_uuid, slot) in &game.slots {
                if let Some(version) = slot.active_version_uuid
                    && active
                        .insert(version, (*game_uuid, *slot_uuid))
                        .is_some_and(|owner| owner != (*game_uuid, *slot_uuid))
                {
                    return Err(anyhow!("Active version has conflicting slot ownership"));
                }
            }
        }
    }

    let mut all_versions = HashSet::new();
    let mut result = Vec::new();
    for game in &catalog.games {
        for slot in &game.slots {
            let mut by_uuid = HashMap::new();
            let mut child_count = HashMap::<Uuid, usize>::new();
            for version in &slot.versions {
                let metadata = &version.version;
                if metadata.game_uuid != game.metadata.uuid
                    || metadata.slot_uuid != Some(slot.metadata.uuid)
                    || !all_versions.insert(metadata.uuid)
                    || by_uuid.insert(metadata.uuid, metadata).is_some()
                {
                    return Err(anyhow!("Invalid or duplicate version in remote catalog"));
                }
            }
            for version in &slot.versions {
                if let Some(parent) = version.version.parent_version_uuid {
                    if !by_uuid.contains_key(&parent) {
                        return Err(anyhow!("Incomplete version parent chain"));
                    }
                    *child_count.entry(parent).or_default() += 1;
                }
                let mut seen = HashSet::new();
                let mut cursor = Some(version.version.uuid);
                // ponytail: O(depth * versions) here; cache validated chains if large histories make previews slow.
                while let Some(uuid) = cursor {
                    if !seen.insert(uuid) {
                        return Err(anyhow!("Cycle in version parent chain"));
                    }
                    cursor = by_uuid.get(&uuid).and_then(|v| v.parent_version_uuid);
                }
            }
            let mut groups = HashMap::<Bucket, Vec<(DateTime<Utc>, Uuid)>>::new();
            for version in &slot.versions {
                let metadata = &version.version;
                if metadata.parent_version_uuid.is_none()
                    || child_count.get(&metadata.uuid).copied().unwrap_or_default() != 1
                    || metadata
                        .tag
                        .as_deref()
                        .is_some_and(|tag| !tag.trim().is_empty())
                    || active.contains_key(&metadata.uuid)
                    || catalog.retired_versions.contains_key(&metadata.uuid)
                {
                    continue;
                }
                if let Some(key) = bucket(metadata.created_at, now) {
                    groups
                        .entry(key)
                        .or_default()
                        .push((metadata.created_at, metadata.uuid));
                }
            }
            for versions in groups.values_mut() {
                versions.sort_unstable();
                result.extend(
                    versions
                        .iter()
                        .take(versions.len().saturating_sub(1))
                        .map(|v| v.1),
                );
            }
        }
    }
    for (version, (game, slot)) in active {
        let belongs = catalog.games.iter().any(|entry| {
            entry.metadata.uuid == game
                && entry.slots.iter().any(|entry| {
                    entry.metadata.uuid == slot
                        && entry
                            .versions
                            .iter()
                            .any(|entry| entry.version.uuid == version)
                })
        });
        if !belongs {
            return Err(anyhow!(
                "An active device version is missing or belongs to another slot"
            ));
        }
    }
    result.sort_unstable();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::{VersionMetadata, VersionReason};
    use crate::sync::{
        DeviceGameState, DeviceMetadata, DeviceSlotState, RemoteGame, RemoteGameMetadata,
        RemoteSlot, RemoteSlotMetadata, RemoteVersionMetadata,
    };
    use chrono::TimeZone;
    use sha2::{Digest, Sha256};
    use tempfile::tempdir;

    #[test]
    fn protects_graph_anchors_and_offline_device_pointer() {
        let now = Utc.with_ymd_and_hms(2026, 9, 22, 12, 0, 0).unwrap();
        let game = Uuid::new_v4();
        let slot = Uuid::new_v4();
        let device = Uuid::new_v4();
        let ids: Vec<_> = (0..6).map(|_| Uuid::new_v4()).collect();
        let mut versions: Vec<RemoteVersionMetadata> = ids
            .iter()
            .enumerate()
            .map(|(index, uuid)| RemoteVersionMetadata {
                version: VersionMetadata {
                    entity: "version".into(),
                    uuid: *uuid,
                    game_uuid: game,
                    slot_uuid: Some(slot),
                    version_number: index as i32 + 1,
                    folder_slug: format!("v{index}"),
                    parent_version_uuid: index.checked_sub(1).map(|i| ids[i]),
                    tag: (index == 2).then(|| "keep".into()),
                    reason: VersionReason::Watcher,
                    created_at: now - chrono::Duration::days(15)
                        + chrono::Duration::minutes(index as i64),
                    content_hash: String::new(),
                    files: vec![],
                },
                origin_device_uuid: device,
            })
            .collect();
        let mut fork_leaf = versions[5].clone();
        fork_leaf.version.uuid = Uuid::new_v4();
        fork_leaf.version.folder_slug = "fork-leaf".into();
        fork_leaf.version.parent_version_uuid = Some(ids[2]);
        versions.push(fork_leaf);
        let mut games = HashMap::new();
        games.insert(
            game,
            DeviceGameState {
                tracked: true,
                active_slot_uuid: Some(slot),
                slots: HashMap::from([(
                    slot,
                    DeviceSlotState {
                        active_version_uuid: Some(ids[3]),
                    },
                )]),
            },
        );
        let state = DeviceState {
            device_uuid: device,
            updated_at: now - chrono::Duration::days(100),
            games,
        };
        let catalog = RemoteCatalog {
            devices: vec![DeviceMetadata {
                device_uuid: device,
                name: "offline".into(),
                platform: "test".into(),
                first_seen: now,
                last_seen: now - chrono::Duration::days(100),
            }],
            states: vec![state.clone()],
            retired_versions: HashMap::new(),
            games: vec![RemoteGame {
                metadata: RemoteGameMetadata {
                    entity: "game".into(),
                    uuid: game,
                    title: "Game".into(),
                    folder_slug: "game".into(),
                    icon: None,
                    created_at: now,
                    updated_at: now,
                },
                tracked_dirs: vec![],
                slots: vec![RemoteSlot {
                    metadata: RemoteSlotMetadata {
                        entity: "slot".into(),
                        uuid: slot,
                        game_uuid: game,
                        name: "Main".into(),
                        folder_slug: "main".into(),
                        created_at: now,
                        updated_at: now,
                    },
                    versions,
                }],
            }],
        };

        assert_eq!(
            plan_retirement(&catalog, &state, now).unwrap(),
            vec![ids[1]]
        );
        let mut incomplete = catalog.clone();
        incomplete.states.clear();
        assert!(plan_retirement(&incomplete, &state, now).is_err());
        let mut broken = catalog;
        broken.games[0].slots[0].versions[1]
            .version
            .parent_version_uuid = Some(Uuid::new_v4());
        assert!(plan_retirement(&broken, &state, now).is_err());
    }

    #[test]
    fn bucket_boundaries_use_elapsed_time_and_utc_calendar() {
        let now = Utc.with_ymd_and_hms(2026, 9, 22, 12, 0, 0).unwrap();
        assert!(bucket(now - chrono::Duration::minutes(59), now).is_none());
        assert!(matches!(
            bucket(now - chrono::Duration::hours(1), now),
            Some(Bucket::Hour(..))
        ));
        assert!(matches!(
            bucket(now - chrono::Duration::hours(24), now),
            Some(Bucket::Day(..))
        ));
        assert!(matches!(
            bucket(
                now - chrono::Duration::days(7) - chrono::Duration::seconds(1),
                now
            ),
            Some(Bucket::Week(..))
        ));
    }

    #[test]
    fn markers_preserve_shared_blobs_and_active_retired_versions() {
        let root = tempdir().unwrap();
        repository::ensure_repository_root(root.path()).unwrap();
        let game = Uuid::new_v4();
        let slot = Uuid::new_v4();
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let bytes = b"shared save";
        let hash = format!("sha256:{:x}", Sha256::digest(bytes));
        let blob = crate::content_store::blob_path(root.path(), &hash).unwrap();
        fs::create_dir_all(blob.parent().unwrap()).unwrap();
        fs::write(&blob, bytes).unwrap();
        let now = Utc::now();
        for uuid in [first, second] {
            repository::write_metadata_json(
                &root
                    .path()
                    .join("games/game/slots/slot/versions")
                    .join(uuid.to_string()),
                &VersionMetadata {
                    entity: "version".into(),
                    uuid,
                    game_uuid: game,
                    slot_uuid: Some(slot),
                    version_number: 1,
                    folder_slug: uuid.to_string(),
                    parent_version_uuid: None,
                    tag: None,
                    reason: VersionReason::Watcher,
                    created_at: now,
                    content_hash: "manifest".into(),
                    files: vec![crate::repository::VersionFileMetadata {
                        tracked_dir_uuid: Uuid::new_v4(),
                        relative_path: "save.dat".into(),
                        size: bytes.len() as u64,
                        sha256: hash.clone(),
                    }],
                },
            )
            .unwrap();
        }
        let marker = RetirementMarker {
            version_uuid: first,
            slot_uuid: slot,
            content_hash: "manifest".into(),
            retired_at: now,
        };
        write_marker(root.path(), &marker).unwrap();
        write_marker(root.path(), &marker).unwrap();
        assert_eq!(read_markers(root.path()).unwrap().len(), 1);
        assert_eq!(
            preview_local_gc(root.path(), &HashSet::new())
                .unwrap()
                .reclaimable_blobs,
            0
        );
        write_marker(
            root.path(),
            &RetirementMarker {
                version_uuid: second,
                ..marker
            },
        )
        .unwrap();
        assert_eq!(
            preview_local_gc(root.path(), &HashSet::new())
                .unwrap()
                .reclaimable_bytes,
            bytes.len() as u64
        );
        assert_eq!(
            preview_local_gc(root.path(), &HashSet::from([first]))
                .unwrap()
                .reclaimable_blobs,
            0
        );
        assert_eq!(
            run_local_gc(root.path(), &HashSet::from([first]))
                .unwrap()
                .reclaimable_blobs,
            0
        );
        assert!(blob.exists());
        assert_eq!(
            run_local_gc(root.path(), &HashSet::new())
                .unwrap()
                .reclaimable_blobs,
            1
        );
        assert!(!blob.exists());
    }

    #[test]
    fn maintenance_refuses_partially_arrived_metadata() {
        let root = tempdir().unwrap();
        repository::ensure_repository_root(root.path()).unwrap();
        fs::create_dir_all(root.path().join("games/partial")).unwrap();
        assert!(validate_catalog_complete(root.path()).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn garbage_collection_refuses_linked_blob() {
        let root = tempdir().unwrap();
        let external = tempdir().unwrap();
        repository::ensure_repository_root(root.path()).unwrap();
        let bytes = b"external save";
        let hash = format!("sha256:{:x}", Sha256::digest(bytes));
        let blob = crate::content_store::blob_path(root.path(), &hash).unwrap();
        fs::create_dir_all(blob.parent().unwrap()).unwrap();
        let target = external.path().join("save.dat");
        fs::write(&target, bytes).unwrap();
        if let Err(error) = std::os::windows::fs::symlink_file(&target, &blob) {
            if error.raw_os_error() == Some(1314) {
                eprintln!("skipping symlink regression: symlink privilege is unavailable");
                return;
            }
            panic!("failed to create test symlink: {error}");
        }
        assert!(run_local_gc(root.path(), &HashSet::new()).is_err());
        assert_eq!(fs::read(&target).unwrap(), bytes);
    }
}
