//! Devices, their published pointers and divergence classification. Copying
//! history lives in `transfer`; the refresh flow lives in the facade.

use crate::error::ErrorKind;
use crate::index::Pointer;
use crate::store::{self, GameRecord, RetirementMarker, Store, VersionMeta, read_json, write_json};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
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
    /// What this device's verified pushes put on the remote, per game
    /// (ADR 0012). `None` from devices that don't publish them: readers then
    /// read every game.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stamps: Option<HashMap<Uuid, GameStamp>>,
}

/// One game as a device last pushed it. Readers use it to skip games that
/// didn't change and to tell whether what the stamp describes has arrived.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GameStamp {
    /// Hash of the pushed game, its markers and which of its versions have
    /// screenshots. Only a hint that something changed.
    pub digest: String,
    pub updated_at: DateTime<Utc>,
    /// Sticky deletion is independent of which metadata timestamp wins.
    #[serde(default)]
    pub deleted: bool,
    /// Deleted slots and the versions their deletion observed.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub deleted_slots: BTreeMap<Uuid, BTreeSet<Uuid>>,
    pub slots: BTreeMap<Uuid, DateTime<Utc>>,
    pub tracked_dirs: BTreeMap<Uuid, DateTime<Utc>>,
    /// Versions no other version descends from: with their ancestors, all of them.
    pub tips: BTreeSet<Uuid>,
    /// Versions edited since they were created (tags), at their `updated_at`.
    pub edited: BTreeMap<Uuid, DateTime<Utc>>,
    /// Versions whose activity (`last_played_at`) has been filled in.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub played: BTreeSet<Uuid>,
    /// Unretired versions whose screenshots were pushed. A screenshot of a
    /// different version cannot satisfy this expectation.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub screenshots: BTreeSet<Uuid>,
}

impl GameStamp {
    /// `game` as this device holds it, with its retirement `markers`, the
    /// versions with a screenshot to push (`screenshots`) and those retired
    /// anywhere (`retired`), whose screenshots aren't pulled.
    pub fn of(
        game: &GameRecord,
        markers: &HashMap<Uuid, RetirementMarker>,
        screenshots: &BTreeSet<Uuid>,
        retired: &HashMap<Uuid, RetirementMarker>,
    ) -> Result<Self> {
        let mut canonical = game.clone();
        canonical.tracked_dirs.sort_by_key(|dir| dir.uuid);
        let markers: BTreeMap<Uuid, &RetirementMarker> = markers
            .iter()
            .filter(|(_, marker)| marker.game_uuid == game.meta.uuid)
            .map(|(uuid, marker)| (*uuid, marker))
            .collect();
        let mut hasher = Sha256::new();
        hasher.update(serde_json::to_vec(&(&canonical, &markers, screenshots))?);
        let parents: HashSet<Uuid> = game.versions().filter_map(|v| v.parent_uuid).collect();
        Ok(Self {
            digest: store::format_hash(&hasher.finalize()),
            updated_at: game.meta.updated_at,
            deleted: game.meta.deleted_at.is_some(),
            deleted_slots: game
                .slots
                .iter()
                .filter(|slot| slot.meta.deleted_at.is_some())
                .map(|slot| {
                    (
                        slot.meta.uuid,
                        slot.meta.seen_versions.iter().copied().collect(),
                    )
                })
                .collect(),
            slots: game
                .slots
                .iter()
                .map(|slot| (slot.meta.uuid, slot.meta.updated_at))
                .collect(),
            tracked_dirs: game
                .tracked_dirs
                .iter()
                .map(|dir| (dir.uuid, dir.updated_at))
                .collect(),
            tips: game
                .versions()
                .map(|v| v.uuid)
                .filter(|uuid| !parents.contains(uuid))
                .collect(),
            edited: game
                .versions()
                .filter(|v| v.updated_at > v.created_at)
                .map(|v| (v.uuid, v.updated_at))
                .collect(),
            played: game
                .versions()
                .filter(|v| v.last_played_at.is_some())
                .map(|v| v.uuid)
                .collect(),
            screenshots: screenshots
                .iter()
                .filter(|uuid| !retired.contains_key(uuid))
                .copied()
                .collect(),
        })
    }

    /// The versions the stamp describes as `game` holds them, when it holds
    /// every one: each tip with its ancestors.
    fn versions_in<'a>(&self, game: &'a GameRecord) -> Option<Vec<&'a VersionMeta>> {
        let versions: HashMap<Uuid, &VersionMeta> = game.versions().map(|v| (v.uuid, v)).collect();
        let mut found = HashMap::new();
        for tip in &self.tips {
            let mut current = Some(*tip);
            let mut chain = HashSet::new();
            while let Some(uuid) = current {
                if !chain.insert(uuid) {
                    return None;
                }
                if found.contains_key(&uuid) {
                    break;
                }
                let version = versions.get(&uuid)?;
                found.insert(uuid, *version);
                current = version.parent_uuid;
            }
        }
        Some(found.into_values().collect())
    }

    /// Whether `game`, as held here with screenshots of the versions in
    /// `shots`, has every screenshot the stamp's versions were pushed with,
    /// leaving out versions retired in `markers`.
    pub fn screenshots_arrived(
        &self,
        game: &GameRecord,
        shots: &BTreeSet<Uuid>,
        markers: &HashMap<Uuid, RetirementMarker>,
    ) -> bool {
        let versions: HashSet<Uuid> = game.versions().map(|v| v.uuid).collect();
        self.screenshots.iter().all(|uuid| {
            markers.contains_key(uuid) || (versions.contains(uuid) && shots.contains(uuid))
        })
    }

    /// Whether `game`, as read from the remote, holds everything the stamp
    /// describes or newer: missing or older metadata means it hasn't arrived.
    pub fn arrived_in(&self, game: &GameRecord) -> bool {
        if self.versions_in(game).is_none() {
            return false;
        }
        let versions: HashMap<Uuid, &VersionMeta> = game.versions().map(|v| (v.uuid, v)).collect();
        game.meta.updated_at >= self.updated_at
            && (!self.deleted || game.meta.deleted_at.is_some())
            && self.deleted_slots.iter().all(|(uuid, seen)| {
                game.slots.iter().any(|slot| {
                    slot.meta.uuid == *uuid
                        && slot.meta.deleted_at.is_some()
                        && seen
                            .iter()
                            .all(|version| slot.meta.seen_versions.contains(version))
                })
            })
            && self.slots.iter().all(|(uuid, at)| {
                game.slots
                    .iter()
                    .any(|slot| slot.meta.uuid == *uuid && slot.meta.updated_at >= *at)
            })
            && self.tracked_dirs.iter().all(|(uuid, at)| {
                game.tracked_dirs
                    .iter()
                    .any(|dir| dir.uuid == *uuid && dir.updated_at >= *at)
            })
            && self
                .edited
                .iter()
                .all(|(uuid, at)| versions.get(uuid).is_some_and(|v| v.updated_at >= *at))
            && self.played.iter().all(|uuid| {
                versions
                    .get(uuid)
                    .is_some_and(|v| v.last_played_at.is_some())
            })
    }
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
            stamps: None,
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SyncSettings {
    pub remote_path: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Divergence {
    InSync,
    FastForward,
    LocalAhead,
    Diverged,
    RemoteOnly,
    LocalOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
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

/// Times the phases of a sync operation and logs them as one line when
/// dropped (early returns included), so a slow remote shows where time goes.
pub(crate) struct Phases {
    operation: &'static str,
    started: Instant,
    last: Instant,
    laps: Vec<String>,
}

impl Phases {
    pub(crate) fn start(operation: &'static str) -> Self {
        let now = Instant::now();
        Self {
            operation,
            started: now,
            last: now,
            laps: Vec::new(),
        }
    }

    /// Ends the phase `name`, which began at the previous lap.
    pub(crate) fn lap(&mut self, name: &str) {
        let now = Instant::now();
        self.laps.push(format!("{name} {:.2?}", now - self.last));
        self.last = now;
    }
}

impl Drop for Phases {
    fn drop(&mut self) {
        log::info!(
            "{} took {:.2?} ({})",
            self.operation,
            self.started.elapsed(),
            self.laps.join(", ")
        );
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
