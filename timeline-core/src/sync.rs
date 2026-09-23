use crate::content_store::{blob_path, verify_blob};
use crate::models::{FullGame, FullSlot};
use crate::repository::{self, GameMetadata, SlotMetadata, TrackedDirMetadata, VersionMetadata};
use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

const DEVICE_FILE: &str = "device.json";
const STATE_FILE: &str = "state.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceMetadata {
    pub device_uuid: Uuid,
    pub name: String,
    pub platform: String,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceRemoval {
    pub device_uuid: Uuid,
    pub removed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceState {
    pub device_uuid: Uuid,
    pub updated_at: DateTime<Utc>,
    pub games: HashMap<Uuid, DeviceGameState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceGameState {
    pub tracked: bool,
    pub active_slot_uuid: Option<Uuid>,
    pub slots: HashMap<Uuid, DeviceSlotState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceSlotState {
    pub active_version_uuid: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteGameMetadata {
    pub entity: String,
    pub uuid: Uuid,
    pub title: String,
    pub folder_slug: String,
    pub icon: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteSlotMetadata {
    pub entity: String,
    pub uuid: Uuid,
    pub game_uuid: Uuid,
    pub name: String,
    pub folder_slug: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteGame {
    pub metadata: RemoteGameMetadata,
    pub tracked_dirs: Vec<TrackedDirMetadata>,
    pub slots: Vec<RemoteSlot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteSlot {
    pub metadata: RemoteSlotMetadata,
    pub versions: Vec<RemoteVersionMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteVersionMetadata {
    #[serde(flatten)]
    pub version: VersionMetadata,
    #[serde(default)]
    pub origin_device_uuid: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteCatalog {
    pub devices: Vec<DeviceMetadata>,
    pub states: Vec<DeviceState>,
    pub games: Vec<RemoteGame>,
    pub retired_versions: HashMap<Uuid, crate::pruning::RetirementMarker>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SyncSettings {
    pub remote_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteGameSummary {
    pub uuid: Uuid,
    pub title: String,
    pub installed: bool,
    pub adopted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GameSyncStatus {
    pub game_uuid: Uuid,
    pub relation: Divergence,
    pub remote_device: Option<String>,
    pub remote_version_uuid: Option<Uuid>,
    pub remote_version_id: Option<i32>,
    pub auto_applied: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncReport {
    pub synced_at: DateTime<Utc>,
    pub games: Vec<GameSyncStatus>,
    pub remote_games: Vec<RemoteGameSummary>,
    pub devices: Vec<DeviceMetadata>,
    pub remote_versions: HashMap<Uuid, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteTip {
    pub device_name: String,
    pub game_state: DeviceGameState,
    pub version_uuid: Option<Uuid>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Divergence {
    InSync,
    FastForward,
    LocalAhead,
    Diverged,
    RemoteOnly,
    LocalOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyDecision {
    AutoApply,
    Prompt,
    CheckpointThenPrompt,
    PreserveBranches,
}

pub fn decide_apply(
    has_local_history: bool,
    relation: Divergence,
    has_unversioned_changes: bool,
    game_running: Option<bool>,
) -> ApplyDecision {
    if !has_local_history {
        return ApplyDecision::AutoApply;
    }
    if relation == Divergence::Diverged {
        return ApplyDecision::PreserveBranches;
    }
    if has_unversioned_changes {
        return ApplyDecision::CheckpointThenPrompt;
    }
    if relation == Divergence::FastForward && game_running == Some(false) {
        ApplyDecision::AutoApply
    } else {
        ApplyDecision::Prompt
    }
}

pub fn load_or_create_device(data_dir: &Path) -> Result<DeviceMetadata> {
    let path = data_dir.join(DEVICE_FILE);
    if path.exists() {
        return repository::read_json_file(&path).context("Failed to load device identity");
    }

    let now = Utc::now();
    let device = DeviceMetadata {
        device_uuid: Uuid::new_v4(),
        name: default_device_name(),
        platform: std::env::consts::OS.to_string(),
        first_seen: now,
        last_seen: now,
    };
    repository::write_json_file(&path, &device).context("Failed to persist device identity")?;
    Ok(device)
}

pub fn save_device(data_dir: &Path, device: &DeviceMetadata) -> Result<()> {
    repository::write_json_file(&data_dir.join(DEVICE_FILE), device)
        .context("Failed to persist device identity")
}

pub fn load_settings(path: &Path) -> Result<SyncSettings> {
    if !path.exists() {
        return Ok(SyncSettings::default());
    }
    repository::read_json_file(path).context("Failed to load sync settings")
}

pub fn save_settings(path: &Path, settings: &SyncSettings) -> Result<()> {
    repository::write_json_file(path, settings).context("Failed to save sync settings")
}

pub fn remove_remote_device(remote_root: &Path, device_uuid: Uuid) -> Result<()> {
    removed_devices(remote_root)?;
    let removed_path = remote_root
        .join("removed-devices")
        .join(format!("{device_uuid}.json"));
    repository::write_json_file(
        &removed_path,
        &DeviceRemoval {
            device_uuid,
            removed_at: Utc::now(),
        },
    )
    .context("Failed to record device removal")?;
    let path = remote_root.join("devices").join(device_uuid.to_string());
    match fs::symlink_metadata(&path) {
        Ok(metadata) if !metadata.is_dir() || crate::fs_utils::is_link_or_reparse(&metadata) => {
            return Err(anyhow!("Remote device entry is not a regular directory"));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    }
    match fs::remove_dir_all(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error)
            .with_context(|| format!("Failed to remove remote device at {}", path.display())),
    }
}

pub fn removed_devices(remote_root: &Path) -> Result<HashSet<Uuid>> {
    let directory = remote_root.join("removed-devices");
    let metadata = match fs::symlink_metadata(&directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashSet::new()),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_dir() || crate::fs_utils::is_link_or_reparse(&metadata) {
        return Err(anyhow!(
            "Removed-device directory is not a regular directory"
        ));
    }
    let mut removed = HashSet::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_str().context("Invalid removed-device filename")?;
        if name.starts_with('.') {
            continue;
        }
        let uuid = name
            .strip_suffix(".json")
            .and_then(|name| Uuid::parse_str(name).ok())
            .context("Invalid removed-device filename")?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if !metadata.is_file() || crate::fs_utils::is_link_or_reparse(&metadata) {
            return Err(anyhow!("Device-removal marker is not a regular file"));
        }
        let marker: DeviceRemoval = repository::read_json_file(&entry.path())?;
        if marker.device_uuid != uuid || !removed.insert(uuid) {
            return Err(anyhow!("Device-removal marker identity mismatch"));
        }
    }
    Ok(removed)
}

pub fn game_is_running(install_path: Option<&str>) -> Option<bool> {
    let install_path = Path::new(install_path?);
    let system =
        sysinfo::System::new_with_specifics(sysinfo::RefreshKind::nothing().with_processes(
            sysinfo::ProcessRefreshKind::nothing().with_exe(sysinfo::UpdateKind::OnlyIfNotSet),
        ));
    if system.processes().is_empty() {
        return None;
    }
    Some(
        system
            .processes()
            .values()
            .filter_map(|process| process.exe())
            .any(|executable| path_is_within(executable, install_path)),
    )
}

impl DeviceState {
    pub fn from_games(
        device_uuid: Uuid,
        games: &[FullGame],
        slots_by_id: &HashMap<i32, FullSlot>,
    ) -> Result<Self> {
        let games = games
            .iter()
            .map(|game| {
                let game_uuid = parse_uuid(game.game.uuid.as_deref(), "game.uuid")?;
                let active_slot_uuid = game
                    .active_slot
                    .as_ref()
                    .map(|slot| Uuid::parse_str(&slot.uuid).context("invalid active slot UUID"))
                    .transpose()?;
                let slots = game
                    .slots
                    .iter()
                    .map(|slot| {
                        let slot_uuid = Uuid::parse_str(&slot.uuid).context("invalid slot UUID")?;
                        let active_version_uuid = slots_by_id
                            .get(&slot.id)
                            .and_then(|full_slot| {
                                full_slot
                                    .versions
                                    .iter()
                                    .find(|version| Some(version.id) == slot.active_version_id)
                            })
                            .and_then(|version| version.uuid.as_deref())
                            .map(|uuid| {
                                Uuid::parse_str(uuid).context("invalid active version UUID")
                            })
                            .transpose()?;
                        Ok((
                            slot_uuid,
                            DeviceSlotState {
                                active_version_uuid,
                            },
                        ))
                    })
                    .collect::<Result<_>>()?;
                Ok((
                    game_uuid,
                    DeviceGameState {
                        tracked: true,
                        active_slot_uuid,
                        slots,
                    },
                ))
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            device_uuid,
            updated_at: Utc::now(),
            games,
        })
    }
}

pub fn push(
    local_root: &Path,
    remote_root: &Path,
    device: &DeviceMetadata,
    state: &DeviceState,
) -> Result<DeviceMetadata> {
    let local_path = fs::canonicalize(local_root).context("Local repository is unavailable")?;
    let remote_path = fs::canonicalize(remote_root).context("Sync folder is unavailable")?;
    if !remote_path.is_dir()
        || local_path.starts_with(&remote_path)
        || remote_path.starts_with(&local_path)
    {
        return Err(anyhow!("Sync folder must be a separate existing directory"));
    }
    repository::ensure_repository_root(local_root).context("Failed to open local repository")?;
    repository::ensure_repository_root(remote_root).context("Failed to initialize sync remote")?;
    if removed_devices(remote_root)?.contains(&device.device_uuid) {
        return Err(anyhow!("This device was removed from the sync folder"));
    }

    let mut games = Vec::new();
    for game_dir in child_dirs(&repository::games_dir(local_root))? {
        let local_game: GameMetadata = match repository::read_metadata_json(&game_dir) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        let remote_game_dir = repository::games_dir(remote_root).join(file_name(&game_dir)?);
        games.push((game_dir, remote_game_dir, local_game));
    }
    let retired = crate::pruning::read_markers(local_root)?;
    for (local_game_dir, _, _) in &games {
        copy_game_blobs(local_root, remote_root, local_game_dir, &retired)?;
    }
    for (local_game_dir, remote_game_dir, local_game) in &games {
        copy_game_metadata(
            local_game_dir,
            remote_game_dir,
            local_game,
            device.device_uuid,
        )?;
    }
    for marker in retired.values() {
        crate::pruning::write_marker(remote_root, marker)?;
    }

    let mut published = device.clone();
    published.last_seen = Utc::now();
    repository::write_json_file(
        &remote_device_file(remote_root, device.device_uuid),
        &published,
    )
    .context("Failed to publish device metadata")?;

    let mut state = state.clone();
    state.device_uuid = device.device_uuid;
    state.updated_at = Utc::now();
    repository::write_json_file(&remote_state_file(remote_root, device.device_uuid), &state)
        .context("Failed to publish device state")?;
    Ok(published)
}

pub fn load_catalog(remote_root: &Path) -> Result<RemoteCatalog> {
    if !remote_root.is_dir() {
        return Err(anyhow!("Sync folder is unavailable"));
    }
    repository::read_metadata_json::<repository::RepositoryMetadata>(remote_root)
        .context("Sync folder is not an initialized repository")?;
    repository::ensure_repository_root(remote_root).context("Failed to open sync remote")?;
    let removed = removed_devices(remote_root)?;
    let mut devices = Vec::new();
    let mut states = Vec::new();
    for device_dir in child_dirs(&remote_root.join("devices"))? {
        if device_dir
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| Uuid::parse_str(name).ok())
            .is_some_and(|uuid| removed.contains(&uuid))
        {
            continue;
        }
        if let Ok(device) = repository::read_json_file(&device_dir.join(DEVICE_FILE)) {
            devices.push(device);
        }
        if let Ok(state) = repository::read_json_file(&device_dir.join(STATE_FILE)) {
            states.push(state);
        }
    }

    let mut games = Vec::new();
    for game_dir in child_dirs(&repository::games_dir(remote_root))? {
        let Ok(metadata) = repository::read_metadata_json(&game_dir) else {
            continue;
        };
        let tracked_dirs = child_dirs(&repository::tracked_dirs_dir(&game_dir))?
            .into_iter()
            .filter_map(|dir| repository::read_metadata_json(&dir).ok())
            .collect();
        let mut slots = Vec::new();
        for slot_dir in child_dirs(&repository::slots_dir(&game_dir))? {
            let Ok(metadata) = repository::read_metadata_json(&slot_dir) else {
                continue;
            };
            let versions = child_dirs(&repository::versions_dir(&slot_dir))?
                .into_iter()
                .filter_map(|dir| repository::read_metadata_json(&dir).ok())
                .collect();
            slots.push(RemoteSlot { metadata, versions });
        }
        games.push(RemoteGame {
            metadata,
            tracked_dirs,
            slots,
        });
    }
    let catalog = RemoteCatalog {
        devices,
        states,
        games,
        retired_versions: crate::pruning::read_markers(remote_root)?,
    };
    crate::pruning::validate_markers(&catalog)?;
    Ok(catalog)
}

pub fn latest_remote_tip(
    catalog: &RemoteCatalog,
    current_device: Uuid,
    game_uuid: Uuid,
) -> Option<RemoteTip> {
    let candidates = catalog
        .states
        .iter()
        .filter(|state| state.device_uuid != current_device)
        .filter(|state| state.games.get(&game_uuid).is_some_and(|game| game.tracked))
        .collect::<Vec<_>>();
    let versions = catalog
        .games
        .iter()
        .find(|game| game.metadata.uuid == game_uuid)?
        .slots
        .iter()
        .flat_map(|slot| &slot.versions)
        .map(|version| (version.version.uuid, version.version.parent_version_uuid))
        .collect::<HashMap<_, _>>();
    let tip = |state: &DeviceState| {
        let game = state.games.get(&game_uuid)?;
        game.active_slot_uuid
            .and_then(|slot| game.slots.get(&slot))
            .and_then(|slot| slot.active_version_uuid)
    };
    let state = candidates
        .iter()
        .copied()
        .filter(|state| {
            let Some(current) = tip(state) else {
                return false;
            };
            !candidates.iter().any(|other| {
                tip(other).is_some_and(|other_tip| {
                    other_tip != current && is_ancestor(current, other_tip, &versions)
                })
            })
        })
        .max_by_key(|state| state.updated_at)
        .or_else(|| candidates.into_iter().max_by_key(|state| state.updated_at))?;
    let game_state = state.games.get(&game_uuid)?.clone();
    let version_uuid = game_state
        .active_slot_uuid
        .and_then(|slot| game_state.slots.get(&slot))
        .and_then(|slot| slot.active_version_uuid);
    let device_name = catalog
        .devices
        .iter()
        .find(|device| device.device_uuid == state.device_uuid)
        .map(|device| device.name.clone())
        .unwrap_or_else(|| "Another device".to_string());
    Some(RemoteTip {
        device_name,
        game_state,
        version_uuid,
    })
}

pub fn remote_heads_diverge(
    catalog: &RemoteCatalog,
    current_device: Uuid,
    game_uuid: Uuid,
) -> bool {
    let Some(game) = catalog
        .games
        .iter()
        .find(|game| game.metadata.uuid == game_uuid)
    else {
        return false;
    };
    let versions = game
        .slots
        .iter()
        .flat_map(|slot| &slot.versions)
        .map(|version| (version.version.uuid, version.version.parent_version_uuid))
        .collect::<HashMap<_, _>>();
    let tips = catalog
        .states
        .iter()
        .filter(|state| state.device_uuid != current_device)
        .filter_map(|state| state.games.get(&game_uuid).filter(|game| game.tracked))
        .filter_map(|game| game.active_slot_uuid.and_then(|slot| game.slots.get(&slot)))
        .filter_map(|slot| slot.active_version_uuid)
        .collect::<Vec<_>>();
    tips.iter().enumerate().any(|(index, left)| {
        tips.iter().skip(index + 1).any(|right| {
            left != right
                && !is_ancestor(*left, *right, &versions)
                && !is_ancestor(*right, *left, &versions)
        })
    })
}

pub fn remote_version_devices(
    catalog: &RemoteCatalog,
    current_device: Uuid,
) -> HashMap<Uuid, String> {
    catalog
        .games
        .iter()
        .flat_map(|game| &game.slots)
        .flat_map(|slot| &slot.versions)
        .filter(|version| version.origin_device_uuid != current_device)
        .map(|version| {
            let name = catalog
                .devices
                .iter()
                .find(|device| device.device_uuid == version.origin_device_uuid)
                .map(|device| device.name.clone())
                .unwrap_or_else(|| "Another device".to_string());
            (version.version.uuid, name)
        })
        .collect()
}

pub fn pull_game(
    local_root: &Path,
    remote_root: &Path,
    game: &RemoteGame,
    retired: &HashMap<Uuid, crate::pruning::RetirementMarker>,
) -> Result<RemoteGame> {
    repository::ensure_repository_root(local_root).context("Failed to open local repository")?;
    repository::with_exclusive_lock(local_root, || {
        let game_slug = repository::validate_folder_slug(
            Some(&game.metadata.folder_slug),
            "remote game folder_slug",
        )?;
        let remote_game_dir = repository::game_dir(remote_root, game_slug);
        let local_game_dir = repository::game_dir(local_root, game_slug);

        let local_game = repository::read_metadata_json::<GameMetadata>(&local_game_dir).ok();
        repository::write_metadata_json(
            &local_game_dir,
            &GameMetadata {
                entity: game.metadata.entity.clone(),
                uuid: game.metadata.uuid,
                title: game.metadata.title.clone(),
                folder_slug: game.metadata.folder_slug.clone(),
                active_slot_uuid: local_game.and_then(|metadata| metadata.active_slot_uuid),
                icon: game.metadata.icon.clone(),
                created_at: game.metadata.created_at,
                updated_at: game.metadata.updated_at,
            },
        )?;

        for tracked in &game.tracked_dirs {
            if tracked.game_uuid != game.metadata.uuid {
                return Err(anyhow!("Remote tracked directory belongs to another game"));
            }
            let tracked_slug = repository::validate_folder_slug(
                Some(&tracked.folder_slug),
                "remote tracked directory folder_slug",
            )?;
            let source = repository::tracked_dir_dir(&remote_game_dir, tracked_slug);
            if repository::metadata_path(&source).exists() {
                repository::write_metadata_json(
                    &repository::tracked_dir_dir(&local_game_dir, tracked_slug),
                    tracked,
                )?;
            }
        }
        let mut pulled_slots = Vec::new();
        for slot in &game.slots {
            if slot.metadata.game_uuid != game.metadata.uuid {
                return Err(anyhow!("Remote slot belongs to another game"));
            }
            let slot_slug = repository::validate_folder_slug(
                Some(&slot.metadata.folder_slug),
                "remote slot folder_slug",
            )?;
            let remote_slot_dir = repository::slot_dir(&remote_game_dir, slot_slug);
            let local_slot_dir = repository::slot_dir(&local_game_dir, slot_slug);
            let local_slot = repository::read_metadata_json::<SlotMetadata>(&local_slot_dir).ok();
            repository::write_metadata_json(
                &local_slot_dir,
                &SlotMetadata {
                    entity: slot.metadata.entity.clone(),
                    uuid: slot.metadata.uuid,
                    game_uuid: slot.metadata.game_uuid,
                    name: slot.metadata.name.clone(),
                    folder_slug: slot.metadata.folder_slug.clone(),
                    active_version_uuid: local_slot
                        .and_then(|metadata| metadata.active_version_uuid),
                    created_at: slot.metadata.created_at,
                    updated_at: slot.metadata.updated_at,
                },
            )?;
            let mut pulled_versions = Vec::new();
            for remote_version in &slot.versions {
                let version = &remote_version.version;
                if version.game_uuid != game.metadata.uuid
                    || version.slot_uuid != Some(slot.metadata.uuid)
                {
                    return Err(anyhow!("Remote version belongs to another game or slot"));
                }
                let version_slug = repository::validate_folder_slug(
                    Some(&version.folder_slug),
                    "remote version folder_slug",
                )?;
                let is_retired = retired.contains_key(&version.uuid);
                if !is_retired && !version_blobs_available(remote_root, version) {
                    continue;
                }
                if !is_retired {
                    for file in &version.files {
                        copy_blob(remote_root, local_root, &file.sha256, file.size)?;
                    }
                }
                let remote_version_dir = repository::version_dir(&remote_slot_dir, version_slug);
                if !repository::metadata_path(&remote_version_dir).exists() {
                    continue;
                }
                repository::write_metadata_json(
                    &repository::version_dir(&local_slot_dir, version_slug),
                    version,
                )?;
                pulled_versions.push(remote_version.clone());
            }
            pulled_slots.push(RemoteSlot {
                metadata: slot.metadata.clone(),
                versions: pulled_versions,
            });
        }
        Ok(RemoteGame {
            metadata: game.metadata.clone(),
            tracked_dirs: game.tracked_dirs.clone(),
            slots: pulled_slots,
        })
    })
}

pub fn classify(
    local_tip: Option<Uuid>,
    remote_tip: Option<Uuid>,
    versions: &HashMap<Uuid, Option<Uuid>>,
) -> Divergence {
    match (local_tip, remote_tip) {
        (None, None) => Divergence::InSync,
        (None, Some(_)) => Divergence::RemoteOnly,
        (Some(_), None) => Divergence::LocalOnly,
        (Some(local), Some(remote)) if local == remote => Divergence::InSync,
        (Some(local), Some(remote)) if is_ancestor(local, remote, versions) => {
            Divergence::FastForward
        }
        (Some(local), Some(remote)) if is_ancestor(remote, local, versions) => {
            Divergence::LocalAhead
        }
        (Some(_), Some(_)) => Divergence::Diverged,
    }
}

pub fn remote_device_file(remote_root: &Path, device_uuid: Uuid) -> PathBuf {
    remote_root
        .join("devices")
        .join(device_uuid.to_string())
        .join(DEVICE_FILE)
}

pub fn remote_state_file(remote_root: &Path, device_uuid: Uuid) -> PathBuf {
    remote_root
        .join("devices")
        .join(device_uuid.to_string())
        .join(STATE_FILE)
}

fn copy_game_blobs(
    local_root: &Path,
    remote_root: &Path,
    local_game_dir: &Path,
    retired: &HashMap<Uuid, crate::pruning::RetirementMarker>,
) -> Result<()> {
    for slot_dir in child_dirs(&repository::slots_dir(local_game_dir))? {
        for version_dir in child_dirs(&repository::versions_dir(&slot_dir))? {
            let Ok(version) = repository::read_metadata_json::<VersionMetadata>(&version_dir)
            else {
                continue;
            };
            if retired.contains_key(&version.uuid) {
                continue;
            }
            for file in &version.files {
                copy_blob(local_root, remote_root, &file.sha256, file.size)?;
            }
        }
    }
    Ok(())
}

fn copy_game_metadata(
    local_game_dir: &Path,
    remote_game_dir: &Path,
    local_game: &GameMetadata,
    device_uuid: Uuid,
) -> Result<()> {
    let remote_game = RemoteGameMetadata {
        entity: local_game.entity.clone(),
        uuid: local_game.uuid,
        title: local_game.title.clone(),
        folder_slug: local_game.folder_slug.clone(),
        icon: local_game.icon.clone(),
        created_at: local_game.created_at,
        updated_at: local_game.updated_at,
    };
    let replace_game = repository::read_metadata_json::<RemoteGameMetadata>(remote_game_dir)
        .map(|current| current.updated_at <= remote_game.updated_at)
        .unwrap_or(true);
    if replace_game {
        repository::write_metadata_json(remote_game_dir, &remote_game)?;
    }

    for tracked_dir in child_dirs(&repository::tracked_dirs_dir(local_game_dir))? {
        let Ok(metadata) = repository::read_metadata_json::<TrackedDirMetadata>(&tracked_dir)
        else {
            continue;
        };
        repository::write_metadata_json(
            &repository::tracked_dirs_dir(remote_game_dir).join(file_name(&tracked_dir)?),
            &metadata,
        )?;
    }
    for slot_dir in child_dirs(&repository::slots_dir(local_game_dir))? {
        let Ok(local_slot) = repository::read_metadata_json::<SlotMetadata>(&slot_dir) else {
            continue;
        };
        let remote_slot_dir = repository::slots_dir(remote_game_dir).join(file_name(&slot_dir)?);
        repository::write_metadata_json(
            &remote_slot_dir,
            &RemoteSlotMetadata {
                entity: local_slot.entity,
                uuid: local_slot.uuid,
                game_uuid: local_slot.game_uuid,
                name: local_slot.name,
                folder_slug: local_slot.folder_slug,
                created_at: local_slot.created_at,
                updated_at: local_slot.updated_at,
            },
        )?;
        for version_dir in child_dirs(&repository::versions_dir(&slot_dir))? {
            let Ok(version) = repository::read_metadata_json::<VersionMetadata>(&version_dir)
            else {
                continue;
            };
            let destination =
                repository::versions_dir(&remote_slot_dir).join(file_name(&version_dir)?);
            write_version_once(
                &destination,
                &RemoteVersionMetadata {
                    version,
                    origin_device_uuid: device_uuid,
                },
            )?;
        }
    }
    Ok(())
}

fn copy_blob(source_root: &Path, destination_root: &Path, hash: &str, size: u64) -> Result<()> {
    let source = verify_blob(source_root, hash, size)?;
    let destination = blob_path(destination_root, hash)?;
    if verify_blob(destination_root, hash, size).is_ok() {
        return Ok(());
    }
    let parent = destination
        .parent()
        .ok_or_else(|| anyhow!("blob destination has no parent"))?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(".sync-{}", Uuid::new_v4()));
    fs::copy(&source, &temp).with_context(|| {
        format!(
            "Failed to copy blob from {} to {}",
            source.display(),
            temp.display()
        )
    })?;
    let copied = fs::metadata(&temp)?.len();
    if copied != size {
        let _ = fs::remove_file(&temp);
        return Err(anyhow!("copied blob has size {copied}, expected {size}"));
    }
    if destination.exists() {
        let _ = fs::remove_file(&temp);
        return verify_blob(destination_root, hash, size).map(|_| ());
    }
    match crate::content_store::atomic_rename_no_replace(&temp, &destination) {
        Ok(()) => verify_blob(destination_root, hash, size).map(|_| ()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let _ = fs::remove_file(&temp);
            verify_blob(destination_root, hash, size).map(|_| ())
        }
        Err(error) => {
            let _ = fs::remove_file(&temp);
            Err(error).context("Failed to publish synchronized blob")
        }
    }
}

fn write_version_once(destination: &Path, version: &RemoteVersionMetadata) -> Result<()> {
    fs::create_dir_all(destination)?;
    let final_path = repository::metadata_path(destination);
    if final_path.exists() {
        let existing: RemoteVersionMetadata = repository::read_json_file(&final_path)?;
        return merge_version_tag(&final_path, existing, version);
    }
    let staging = destination.join(format!(".metadata-sync-{}.json", Uuid::new_v4()));
    repository::write_json_file(&staging, version)?;
    match crate::content_store::atomic_rename_no_replace(&staging, &final_path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let _ = fs::remove_file(&staging);
            let existing: RemoteVersionMetadata = repository::read_json_file(&final_path)?;
            merge_version_tag(&final_path, existing, version)
        }
        Err(error) => {
            let _ = fs::remove_file(&staging);
            Err(error).context("Failed to publish remote version metadata")
        }
    }
}

fn merge_version_tag(
    path: &Path,
    mut existing: RemoteVersionMetadata,
    incoming: &RemoteVersionMetadata,
) -> Result<()> {
    let mut old_identity = existing.version.clone();
    let mut new_identity = incoming.version.clone();
    old_identity.tag = None;
    new_identity.tag = None;
    if old_identity != new_identity {
        return Err(anyhow!("Version UUID was reused with different metadata"));
    }
    // A published tag is a retention anchor. A stale device must never erase it.
    if existing.version.tag.as_deref().is_none_or(str::is_empty)
        && incoming
            .version
            .tag
            .as_deref()
            .is_some_and(|tag| !tag.trim().is_empty())
    {
        existing.version.tag = incoming.version.tag.clone();
        repository::write_json_file(path, &existing)?;
    }
    Ok(())
}

fn version_blobs_available(root: &Path, version: &VersionMetadata) -> bool {
    version
        .files
        .iter()
        .all(|file| verify_blob(root, &file.sha256, file.size).is_ok())
}

fn is_ancestor(
    candidate: Uuid,
    mut descendant: Uuid,
    versions: &HashMap<Uuid, Option<Uuid>>,
) -> bool {
    let mut visited = HashSet::new();
    while visited.insert(descendant) {
        let Some(Some(parent)) = versions.get(&descendant) else {
            return false;
        };
        if *parent == candidate {
            return true;
        }
        descendant = *parent;
    }
    false
}

fn child_dirs(parent: &Path) -> Result<Vec<PathBuf>> {
    if !parent.exists() {
        return Ok(Vec::new());
    }
    Ok(fs::read_dir(parent)?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            entry
                .file_type()
                .ok()
                .filter(|kind| kind.is_dir())
                .map(|_| entry.path())
        })
        .collect())
}

fn file_name(path: &Path) -> Result<&std::ffi::OsStr> {
    path.file_name()
        .ok_or_else(|| anyhow!("path has no file name: {}", path.display()))
}

fn parse_uuid(value: Option<&str>, field: &str) -> Result<Uuid> {
    Uuid::parse_str(value.ok_or_else(|| anyhow!("{field} is missing"))?)
        .with_context(|| format!("{field} is invalid"))
}

fn path_is_within(path: &Path, parent: &Path) -> bool {
    #[cfg(windows)]
    {
        let path = path.to_string_lossy().replace('/', "\\").to_lowercase();
        let mut parent = parent.to_string_lossy().replace('/', "\\").to_lowercase();
        parent.push('\\');
        path == parent.trim_end_matches('\\') || path.starts_with(&parent)
    }
    #[cfg(not(windows))]
    {
        path == parent || path.starts_with(parent)
    }
}

fn default_device_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "Device".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::{VersionFileMetadata, VersionReason};
    use sha2::{Digest, Sha256};
    use tempfile::tempdir;

    #[test]
    fn device_identity_is_stable_and_publishes_state() {
        let local = tempdir().unwrap();
        let remote = tempdir().unwrap();
        repository::ensure_repository_root(local.path()).unwrap();
        let first = load_or_create_device(local.path()).unwrap();
        assert_eq!(load_or_create_device(local.path()).unwrap(), first);
        let state = DeviceState {
            device_uuid: first.device_uuid,
            updated_at: Utc::now(),
            games: HashMap::new(),
        };
        let published = push(local.path(), remote.path(), &first, &state).unwrap();
        let stored: DeviceMetadata =
            repository::read_json_file(&remote_device_file(remote.path(), first.device_uuid))
                .unwrap();
        let stored_state: DeviceState =
            repository::read_json_file(&remote_state_file(remote.path(), first.device_uuid))
                .unwrap();
        assert_eq!(stored, published);
        assert_eq!(stored_state.device_uuid, first.device_uuid);
    }

    #[test]
    fn classifies_parent_chains_without_collapsing_divergence() {
        let root = Uuid::new_v4();
        let local = Uuid::new_v4();
        let remote = Uuid::new_v4();
        let mut versions = HashMap::from([(root, None), (local, Some(root))]);
        assert_eq!(
            classify(Some(root), Some(local), &versions),
            Divergence::FastForward
        );
        versions.insert(remote, Some(root));
        assert_eq!(
            classify(Some(local), Some(remote), &versions),
            Divergence::Diverged
        );
    }

    #[test]
    fn catalog_ignores_partial_versions_and_pull_retries_bad_blobs() {
        let local = tempdir().unwrap();
        let remote = tempdir().unwrap();
        repository::ensure_repository_root(remote.path()).unwrap();
        let game_uuid = Uuid::new_v4();
        let slot_uuid = Uuid::new_v4();
        let game_slug = "game__g_00000000";
        let slot_slug = "slot__s_00000000";
        let version_slug = "v001__v_00000000";
        let game_dir = repository::game_dir(remote.path(), game_slug);
        let slot_dir = repository::slot_dir(&game_dir, slot_slug);
        repository::write_metadata_json(
            &game_dir,
            &RemoteGameMetadata {
                entity: "game".into(),
                uuid: game_uuid,
                title: "Game".into(),
                folder_slug: game_slug.into(),
                icon: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
        )
        .unwrap();
        repository::write_metadata_json(
            &slot_dir,
            &RemoteSlotMetadata {
                entity: "slot".into(),
                uuid: slot_uuid,
                game_uuid,
                name: "Main".into(),
                folder_slug: slot_slug.into(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
        )
        .unwrap();
        fs::create_dir_all(repository::versions_dir(&slot_dir).join("partial")).unwrap();
        let bytes = b"save";
        let hash = format!("sha256:{:x}", Sha256::digest(bytes));
        let version = VersionMetadata {
            entity: "version".into(),
            uuid: Uuid::new_v4(),
            game_uuid,
            slot_uuid: Some(slot_uuid),
            version_number: 1,
            folder_slug: version_slug.into(),
            parent_version_uuid: None,
            tag: None,
            reason: VersionReason::Initial,
            created_at: Utc::now(),
            content_hash: hash.clone(),
            files: vec![VersionFileMetadata {
                tracked_dir_uuid: Uuid::new_v4(),
                relative_path: "save.dat".into(),
                size: bytes.len() as u64,
                sha256: hash.clone(),
            }],
        };
        repository::write_metadata_json(
            &repository::version_dir(&slot_dir, version_slug),
            &RemoteVersionMetadata {
                version,
                origin_device_uuid: Uuid::new_v4(),
            },
        )
        .unwrap();
        let bad_blob = blob_path(remote.path(), &hash).unwrap();
        fs::create_dir_all(bad_blob.parent().unwrap()).unwrap();
        fs::write(&bad_blob, b"nope").unwrap();
        let conflicted = remote
            .path()
            .join("devices")
            .join(Uuid::new_v4().to_string());
        fs::create_dir_all(&conflicted).unwrap();
        fs::write(conflicted.join("state (1).json"), b"not json").unwrap();

        let catalog = load_catalog(remote.path()).unwrap();
        assert_eq!(catalog.games[0].slots[0].versions.len(), 1);
        let pulled = pull_game(
            local.path(),
            remote.path(),
            &catalog.games[0],
            &catalog.retired_versions,
        )
        .unwrap();
        assert!(pulled.slots[0].versions.is_empty());
        assert!(
            !repository::metadata_path(&repository::version_dir(
                &repository::slot_dir(&repository::game_dir(local.path(), game_slug), slot_slug),
                version_slug
            ))
            .exists()
        );
    }

    #[test]
    fn apply_state_one_without_local_history_is_automatic() {
        assert_eq!(
            decide_apply(false, Divergence::RemoteOnly, false, None),
            ApplyDecision::AutoApply
        );
    }

    #[test]
    fn apply_state_two_safe_fast_forward_is_automatic() {
        assert_eq!(
            decide_apply(true, Divergence::FastForward, false, Some(false)),
            ApplyDecision::AutoApply
        );
    }

    #[test]
    fn apply_state_three_running_or_unknown_prompts() {
        assert_eq!(
            decide_apply(true, Divergence::FastForward, false, Some(true)),
            ApplyDecision::Prompt
        );
        assert_eq!(
            decide_apply(true, Divergence::FastForward, false, None),
            ApplyDecision::Prompt
        );
    }

    #[test]
    fn apply_state_four_unversioned_changes_checkpoint_then_prompt() {
        assert_eq!(
            decide_apply(true, Divergence::FastForward, true, Some(false)),
            ApplyDecision::CheckpointThenPrompt
        );
    }

    #[test]
    fn apply_state_five_divergence_preserves_both_branches() {
        assert_eq!(
            decide_apply(true, Divergence::Diverged, false, Some(false)),
            ApplyDecision::PreserveBranches
        );
    }
}
