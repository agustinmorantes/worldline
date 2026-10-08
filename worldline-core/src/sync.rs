//! Devices, their published pointers and divergence classification. Copying
//! history lives in `transfer`; the refresh flow lives in the facade.

use crate::error::ErrorKind;
use crate::index::Pointer;
use crate::store::{self, GameRecord, Store, read_json, write_json};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceMetadata {
    pub device_uuid: Uuid,
    pub name: String,
    pub platform: String,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    /// Chosen by the user, never detected; `None` reads as desktop.
    #[serde(default)]
    pub kind: Option<DeviceKind>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DeviceKind {
    Desktop,
    Laptop,
    Handheld,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceState {
    pub device_uuid: Uuid,
    pub updated_at: DateTime<Utc>,
    pub games: HashMap<Uuid, DeviceGameState>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceGameState {
    pub active_slot_uuid: Option<Uuid>,
    pub slots: HashMap<Uuid, DeviceSlotState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceSlotState {
    pub active_version_uuid: Option<Uuid>,
}

impl DeviceState {
    pub fn from_pointers(device_uuid: Uuid, pointers: &[Pointer]) -> Self {
        let mut games: HashMap<Uuid, DeviceGameState> = HashMap::new();
        for pointer in pointers {
            let game = games.entry(pointer.game).or_default();
            if pointer.active {
                game.active_slot_uuid = Some(pointer.slot);
            }
            game.slots.insert(
                pointer.slot,
                DeviceSlotState {
                    active_version_uuid: pointer.version,
                },
            );
        }
        Self {
            device_uuid,
            updated_at: Utc::now(),
            games,
        }
    }

    pub fn slot_tip(&self, game: Uuid, slot: Uuid) -> Option<Uuid> {
        self.games.get(&game)?.slots.get(&slot)?.active_version_uuid
    }

    /// Every version this device points at.
    pub fn pointed_versions(&self) -> impl Iterator<Item = Uuid> + '_ {
        self.games
            .values()
            .flat_map(|game| game.slots.values())
            .filter_map(|slot| slot.active_version_uuid)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SyncSettings {
    pub remote_path: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteGameSummary {
    pub uuid: Uuid,
    pub title: String,
    pub installed: bool,
    pub adopted: bool,
    /// Name of the device that created its newest version, when known.
    pub source_device: Option<String>,
    /// Versions across all its slots, retired ones included.
    pub version_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GameSyncStatus {
    pub game_uuid: Uuid,
    /// Relation of the local active slot to the other devices.
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
    /// Version UUID → name of the device that created it, for other devices' versions.
    pub remote_versions: HashMap<Uuid, String>,
    /// Games or versions that could not be synced this time, and why.
    pub problems: Vec<String>,
}

pub fn load_or_create_device(data_dir: &Path) -> Result<DeviceMetadata> {
    let path = data_dir.join("device.json");
    if path.exists() {
        return read_json(&path).context("Failed to load device identity");
    }
    let now = Utc::now();
    let device = DeviceMetadata {
        device_uuid: Uuid::new_v4(),
        name: std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .ok()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| "Device".to_string()),
        platform: std::env::consts::OS.to_string(),
        first_seen: now,
        last_seen: now,
        kind: None,
    };
    save_device(data_dir, &device)?;
    Ok(device)
}

pub fn save_device(data_dir: &Path, device: &DeviceMetadata) -> Result<()> {
    write_json(&data_dir.join("device.json"), device).context("Failed to persist device identity")
}

pub fn load_settings(path: &Path) -> Result<SyncSettings> {
    if !path.exists() {
        return Ok(SyncSettings::default());
    }
    read_json(path).context("Failed to load sync settings")
}

pub fn save_settings(path: &Path, settings: &SyncSettings) -> Result<()> {
    write_json(path, settings).context("Failed to save sync settings")
}

/// The sync remote: a store plus the devices' published identity and pointers.
pub struct Remote {
    pub store: Store,
    pub devices: Vec<DeviceMetadata>,
    pub states: Vec<DeviceState>,
    /// A `state.json` that can't be read, or that hasn't arrived for a
    /// registered device. Its device's pointers are unknown, so pruning and
    /// GC must not run.
    pub unreadable_state: Option<PathBuf>,
}

impl Remote {
    /// Opens (initializing when empty) a remote that must not overlap `data_dir`.
    pub fn open(path: &Path, data_dir: &Path) -> Result<Self> {
        let remote = std::fs::canonicalize(path).context(ErrorKind::SyncFolderUnreachable)?;
        let local =
            std::fs::canonicalize(data_dir).context("Local data directory is unavailable")?;
        if !remote.is_dir() {
            bail!("Sync folder must be a directory");
        }
        if remote.starts_with(&local) || local.starts_with(&remote) {
            bail!("The sync folder must not overlap local application data");
        }
        let store = Store::open(path).context("Failed to open the sync folder")?;
        let mut devices = Vec::new();
        let mut states = Vec::new();
        let mut unreadable_state = None;
        for dir in store::entries(&path.join("devices"))? {
            let Some(uuid) = dir
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| Uuid::parse_str(n).ok())
            else {
                continue;
            };
            let device_path = dir.join("device.json");
            if let Ok(device) = read_json::<DeviceMetadata>(&device_path)
                && device.device_uuid == uuid
            {
                devices.push(device);
            }
            let state_path = dir.join("state.json");
            match read_json::<DeviceState>(&state_path) {
                Ok(state) if state.device_uuid == uuid => states.push(state),
                _ if state_path.exists() || device_path.exists() => {
                    unreadable_state = Some(state_path)
                }
                _ => {}
            }
        }
        devices.sort_by(|a, b| a.name.cmp(&b.name).then(a.device_uuid.cmp(&b.device_uuid)));
        Ok(Self {
            store,
            devices,
            states,
            unreadable_state,
        })
    }

    fn device_dir(&self, device: Uuid) -> PathBuf {
        self.store.root().join("devices").join(device.to_string())
    }

    /// Publishes this device's identity and pointers.
    pub fn publish(&self, device: &DeviceMetadata, state: &DeviceState) -> Result<DeviceMetadata> {
        let mut published = device.clone();
        published.last_seen = Utc::now();
        let dir = self.device_dir(device.device_uuid);
        write_json(&dir.join("device.json"), &published)?;
        write_json(&dir.join("state.json"), state)?;
        Ok(published)
    }

    /// Stops protecting a device's pointers. A device that comes back simply
    /// re-registers (ADR 0002: there are no removal markers).
    pub fn remove_device(&self, device: Uuid) -> Result<()> {
        let dir = self.device_dir(device);
        match std::fs::remove_dir_all(&dir) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                Err(error).with_context(|| format!("Failed to remove {}", dir.display()))
            }
            _ => Ok(()),
        }
    }

    pub fn device_name(&self, device: Uuid) -> String {
        self.devices
            .iter()
            .find(|d| d.device_uuid == device)
            .map(|d| d.name.clone())
            .unwrap_or_else(|| "Another device".to_string())
    }

    /// Other devices' states, newest first.
    pub fn other_states(&self, me: Uuid) -> Vec<&DeviceState> {
        let mut states: Vec<_> = self.states.iter().filter(|s| s.device_uuid != me).collect();
        states.sort_by_key(|state| std::cmp::Reverse(state.updated_at));
        states
    }
}

pub type Parents = HashMap<Uuid, Option<Uuid>>;

pub fn parents(game: &GameRecord) -> Parents {
    game.versions().map(|v| (v.uuid, v.parent_uuid)).collect()
}

/// Whether `ancestor` is a strict ancestor of `descendant`.
pub fn is_ancestor(ancestor: Uuid, mut descendant: Uuid, parents: &Parents) -> bool {
    let mut steps = 0;
    while let Some(Some(parent)) = parents.get(&descendant) {
        if *parent == ancestor {
            return true;
        }
        descendant = *parent;
        steps += 1;
        if steps > parents.len() {
            return false;
        }
    }
    false
}

/// Classifies a slot's local tip against the tips other devices published
/// for the same slot. Tips behind the local one (stale devices) never block a
/// fast-forward. Returns the fast-forward or remote-only target.
pub fn classify(
    local: Option<Uuid>,
    remote_tips: &[Uuid],
    parents: &Parents,
) -> (Divergence, Option<Uuid>) {
    let tips: Vec<Uuid> = remote_tips
        .iter()
        .copied()
        .filter(|tip| parents.contains_key(tip))
        .collect();
    let Some(local) = local else {
        return match tips.first() {
            Some(first) => (Divergence::RemoteOnly, Some(*first)),
            None => (Divergence::InSync, None),
        };
    };
    if tips.is_empty() {
        return (Divergence::LocalOnly, None);
    }
    let ahead: Vec<Uuid> = tips
        .iter()
        .copied()
        .filter(|tip| *tip != local && !is_ancestor(*tip, local, parents))
        .collect();
    if ahead.is_empty() {
        let relation = if tips.contains(&local) {
            Divergence::InSync
        } else {
            Divergence::LocalAhead
        };
        return (relation, None);
    }
    if ahead.iter().any(|tip| !is_ancestor(local, *tip, parents)) {
        return (Divergence::Diverged, None);
    }
    let newest = ahead.iter().copied().find(|candidate| {
        ahead
            .iter()
            .all(|tip| tip == candidate || is_ancestor(*tip, *candidate, parents))
    });
    match newest {
        Some(target) => (Divergence::FastForward, Some(target)),
        None => (Divergence::Diverged, None),
    }
}

/// Whether a process runs from inside `install_path`. `None` means unknown,
/// which callers treat as running (fail closed).
pub fn game_is_running(install_path: Option<&str>) -> Option<bool> {
    let install_path = install_path?;
    let system =
        sysinfo::System::new_with_specifics(sysinfo::RefreshKind::nothing().with_processes(
            sysinfo::ProcessRefreshKind::nothing().with_exe(sysinfo::UpdateKind::OnlyIfNotSet),
        ));
    let executables: Vec<_> = system
        .processes()
        .values()
        .filter_map(|p| p.exe())
        .collect();
    if executables.is_empty() {
        return None;
    }
    Some(
        executables
            .iter()
            .any(|exe| crate::detection::running::runs_from(exe, install_path)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification_ignores_stale_devices_and_detects_divergence() {
        let (root, a, b, c) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        // root → a → b, root → c
        let parents = Parents::from([(root, None), (a, Some(root)), (b, Some(a)), (c, Some(root))]);
        assert_eq!(
            classify(Some(a), &[b], &parents),
            (Divergence::FastForward, Some(b))
        );
        // A stale third device at `root` doesn't block the fast-forward.
        assert_eq!(
            classify(Some(a), &[root, b], &parents),
            (Divergence::FastForward, Some(b))
        );
        assert_eq!(
            classify(Some(a), &[a, b], &parents),
            (Divergence::FastForward, Some(b))
        );
        assert_eq!(classify(Some(b), &[a], &parents).0, Divergence::LocalAhead);
        assert_eq!(
            classify(Some(b), &[b, root], &parents).0,
            Divergence::InSync
        );
        assert_eq!(classify(Some(a), &[c], &parents).0, Divergence::Diverged);
        assert_eq!(
            classify(Some(root), &[b, c], &parents).0,
            Divergence::Diverged
        );
        assert_eq!(
            classify(None, &[b], &parents),
            (Divergence::RemoteOnly, Some(b))
        );
        assert_eq!(classify(Some(a), &[], &parents).0, Divergence::LocalOnly);
        // A tip that hasn't arrived is ignored.
        assert_eq!(
            classify(Some(a), &[Uuid::new_v4()], &parents).0,
            Divergence::LocalOnly
        );
    }
}
