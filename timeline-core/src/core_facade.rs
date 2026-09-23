use crate::detection::{self, Command, CommandSender, NotificationReceiver};
use crate::install_scanner::InstallScannerItem;
use crate::models::*;
use crate::persistence::{GameRepository, ScanRootsRepository, VersionRepository};
use crate::repository::VersionReason;
use anyhow::{Context, Result, anyhow};
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use uuid::Uuid;

/// How long a caller waits for the watcher to acknowledge a pause before giving
/// up and proceeding unsuppressed. Generous, because it should only ever be hit
/// by a wedged watcher thread: a dead one drops the acknowledgement channel and
/// fails the wait immediately. A spurious version is a far better outcome than a
/// restore that hangs the UI.
const WATCHER_PAUSE_ACK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// How long to stay paused after mutating tracked directories, so the OS has time
/// to deliver events for our own writes while they are still being discarded.
/// Chosen to comfortably exceed filesystem notification latency without being
/// noticeable in a restore; the debouncer is rebuilt on `Resume` as well, which
/// covers anything still buffered when this elapses.
const WATCHER_SETTLE_DELAY: std::time::Duration = std::time::Duration::from_millis(250);

struct PreparedRestore {
    restore_roots: std::collections::BTreeMap<PathBuf, Vec<glob::Pattern>>,
    files: Vec<(PathBuf, PathBuf, PathBuf)>,
}

enum PreparedSlotActivation {
    Restore(PreparedRestore),
    Clear,
}

pub struct TimelineCore {
    games_repo: Arc<dyn GameRepository>,
    versions_repo: Arc<dyn VersionRepository>,
    scan_roots_repo: Arc<dyn ScanRootsRepository>,
    cmd_tx: CommandSender,
    pub working_base: PathBuf,
    pub icons_dir: PathBuf,
    data_dir: PathBuf,
    monitoring_settings_path: PathBuf,
    pruning_settings_path: PathBuf,
    sync_settings_path: PathBuf,
    device_metadata: RwLock<crate::sync::DeviceMetadata>,
    sync_lock: Mutex<()>,
}

impl TimelineCore {
    pub fn new(
        games_repo: Arc<dyn GameRepository>,
        versions_repo: Arc<dyn VersionRepository>,
        scan_roots_repo: Arc<dyn ScanRootsRepository>,
        working_base: PathBuf,
        icons_dir: PathBuf,
        new_version_seconds: Option<f32>,
    ) -> Result<(Self, NotificationReceiver)> {
        crate::repository::ensure_repository_root(&working_base)
            .context("Failed to initialize repository root")?;

        let data_dir = if working_base
            .file_name()
            .is_some_and(|name| name == "repository")
        {
            working_base.parent().unwrap_or(&working_base).to_path_buf()
        } else {
            working_base.clone()
        };
        let monitoring_settings_path = data_dir.join("monitoring_settings.json");
        let pruning_settings_path = data_dir.join("pruning_settings.json");
        let sync_settings_path = data_dir.join("sync_settings.json");
        let device_metadata = crate::sync::load_or_create_device(&data_dir)?;
        let mut monitoring_settings = load_monitoring_settings(&monitoring_settings_path)?;
        if let Some(seconds) = new_version_seconds {
            monitoring_settings.snapshot_debounce_seconds = seconds;
        }

        let (cmd_tx, notif_rx) = detection::init(
            games_repo.clone(),
            versions_repo.clone(),
            working_base.clone(),
            monitoring_settings,
        )?;

        Ok((
            Self {
                games_repo,
                versions_repo,
                scan_roots_repo,
                cmd_tx,
                working_base,
                icons_dir,
                data_dir,
                monitoring_settings_path,
                pruning_settings_path,
                sync_settings_path,
                device_metadata: RwLock::new(device_metadata),
                sync_lock: Mutex::new(()),
            },
            notif_rx,
        ))
    }

    pub fn device_metadata(&self) -> crate::sync::DeviceMetadata {
        self.device_metadata
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn update_device_name(&self, name: &str) -> Result<crate::sync::DeviceMetadata> {
        let name = name.trim();
        if name.is_empty() {
            return Err(anyhow!("Device name cannot be empty"));
        }
        let mut device = self
            .device_metadata
            .write()
            .map_err(|_| anyhow!("Device metadata lock poisoned"))?;
        device.name = name.to_string();
        crate::sync::save_device(&self.data_dir, &device)?;
        Ok(device.clone())
    }

    pub fn get_sync_settings(&self) -> Result<crate::sync::SyncSettings> {
        crate::sync::load_settings(&self.sync_settings_path)
    }

    pub fn get_pruning_settings(&self) -> Result<crate::pruning::PruningSettings> {
        crate::pruning::load_settings(&self.pruning_settings_path)
    }

    pub fn update_pruning_settings(
        &self,
        settings: crate::pruning::PruningSettings,
    ) -> Result<crate::pruning::PruningSettings> {
        if settings.automatic
            && let Some(remote) = self.get_sync_settings()?.remote_path
            && crate::sync::load_catalog(Path::new(&remote))?
                .devices
                .iter()
                .any(|device| device.device_uuid != self.device_metadata().device_uuid)
        {
            return Err(anyhow!(
                "Automatic pruning requires other sync devices to be removed"
            ));
        }
        crate::pruning::save_settings(&self.pruning_settings_path, settings)?;
        Ok(settings)
    }

    pub fn preview_pruning(&self) -> Result<crate::pruning::PruningPreview> {
        let (catalog, state, _) = self.pruning_context(false)?;
        let active = state
            .games
            .values()
            .flat_map(|game| game.slots.values())
            .filter_map(|slot| slot.active_version_uuid)
            .collect();
        Ok(crate::pruning::PruningPreview {
            retire_versions: crate::pruning::plan_retirement(&catalog, &state, chrono::Utc::now())?,
            local_gc: crate::pruning::preview_local_gc(&self.working_base, &active)?,
        })
    }

    pub fn retired_versions(&self) -> Result<Vec<Uuid>> {
        let mut versions = crate::pruning::read_markers(&self.working_base)?
            .into_keys()
            .collect::<Vec<_>>();
        versions.sort_unstable();
        Ok(versions)
    }

    pub fn run_pruning(&self) -> Result<crate::pruning::PruningRunReport> {
        let _sync_guard = self
            .sync_lock
            .lock()
            .map_err(|_| anyhow!("Sync lock poisoned"))?;
        if self.get_sync_settings()?.remote_path.is_some() {
            self.push_sync_unlocked()?;
        }
        crate::repository::with_exclusive_lock(&self.working_base, || {
            let (catalog, state, remote) = self.pruning_context(true)?;
            if remote.is_some()
                && catalog
                    .devices
                    .iter()
                    .any(|device| device.device_uuid != state.device_uuid)
            {
                return Err(anyhow!(
                    "Pruning requires every other sync device to be explicitly removed"
                ));
            }
            let retired_versions =
                crate::pruning::plan_retirement(&catalog, &state, chrono::Utc::now())?;
            for uuid in &retired_versions {
                let version = catalog
                    .games
                    .iter()
                    .flat_map(|game| &game.slots)
                    .flat_map(|slot| &slot.versions)
                    .find(|version| version.version.uuid == *uuid)
                    .context("Planned version is missing from catalog")?;
                let marker = crate::pruning::RetirementMarker {
                    version_uuid: *uuid,
                    slot_uuid: version
                        .version
                        .slot_uuid
                        .context("Version is missing slot UUID")?,
                    content_hash: version.version.content_hash.clone(),
                    retired_at: chrono::Utc::now(),
                };
                if let Some(root) = &remote {
                    crate::pruning::write_marker(root, &marker)?;
                }
                crate::pruning::write_marker(&self.working_base, &marker)?;
            }
            let active = self
                .device_state(&self.games_repo.get_games()?)?
                .games
                .values()
                .flat_map(|game| game.slots.values())
                .filter_map(|slot| slot.active_version_uuid)
                .collect();
            let local_gc = crate::pruning::run_local_gc_unlocked(&self.working_base, &active)?;
            let remote_gc = remote
                .as_deref()
                .map(|root| {
                    crate::pruning::validate_catalog_complete(root)?;
                    let current = crate::sync::load_catalog(root)?;
                    if current
                        .devices
                        .iter()
                        .any(|device| device.device_uuid != state.device_uuid)
                    {
                        return Err(anyhow!(
                            "Remote cleanup requires every other sync device to be explicitly removed"
                        ));
                    }
                    let mut remote_active = active.clone();
                    for state in &current.states {
                        remote_active.extend(
                            state
                                .games
                                .values()
                                .flat_map(|game| game.slots.values())
                                .filter_map(|slot| slot.active_version_uuid),
                        );
                    }
                    crate::pruning::run_local_gc(root, &remote_active)
                })
                .transpose()?;
            Ok(crate::pruning::PruningRunReport {
                retired_versions,
                local_gc,
                remote_gc,
            })
        })
    }

    fn pruning_context(
        &self,
        strict: bool,
    ) -> Result<(
        crate::sync::RemoteCatalog,
        crate::sync::DeviceState,
        Option<PathBuf>,
    )> {
        if strict {
            crate::pruning::validate_catalog_complete(&self.working_base)?;
        }
        let games = self.games_repo.get_games()?;
        let state = self.device_state(&games)?;
        let local_catalog = crate::sync::load_catalog(&self.working_base)?;
        let settings = self.get_sync_settings()?;
        let remote = settings.remote_path.map(PathBuf::from);
        let mut catalog = if let Some(remote) = &remote {
            if strict {
                crate::pruning::validate_catalog_complete(remote)?;
            }
            crate::sync::load_catalog(remote)?
        } else {
            crate::sync::RemoteCatalog {
                devices: Vec::new(),
                states: Vec::new(),
                games: Vec::new(),
                retired_versions: Default::default(),
            }
        };
        crate::pruning::merge_local_history(&mut catalog, local_catalog)?;
        Ok((catalog, state, remote))
    }

    pub fn update_sync_settings(
        &self,
        settings: crate::sync::SyncSettings,
    ) -> Result<crate::sync::SyncSettings> {
        let _sync_guard = self
            .sync_lock
            .lock()
            .map_err(|_| anyhow!("Sync lock poisoned"))?;
        if let Some(path) = settings.remote_path.as_deref()
            && path.trim().is_empty()
        {
            return Err(anyhow!("Remote path cannot be empty"));
        }
        if let Some(path) = settings.remote_path.as_deref() {
            let remote = std::fs::canonicalize(path)
                .context("Sync folder must already exist and be reachable")?;
            if !remote.is_dir() {
                return Err(anyhow!("Sync folder must be a directory"));
            }
            let data_dir = std::fs::canonicalize(&self.data_dir)?;
            if remote.starts_with(&data_dir) || data_dir.starts_with(&remote) {
                return Err(anyhow!(
                    "The sync folder must not overlap local application data"
                ));
            }
        }
        crate::sync::save_settings(&self.sync_settings_path, &settings)?;
        Ok(settings)
    }

    pub fn refresh_sync(&self) -> Result<crate::sync::SyncReport> {
        let _guard = self
            .sync_lock
            .lock()
            .map_err(|_| anyhow!("Sync lock poisoned"))?;
        let settings = self.get_sync_settings()?;
        let remote_root = PathBuf::from(
            settings
                .remote_path
                .as_deref()
                .context("Sync remote is not configured")?,
        );
        let device = self.device_metadata();
        let initial_games = self.games_repo.get_games()?;
        self.push_sync_unlocked()?;

        let catalog = crate::sync::load_catalog(&remote_root)?;
        for marker in catalog.retired_versions.values() {
            crate::pruning::write_marker(&self.working_base, marker)?;
        }
        let mut local_uuids = initial_games
            .iter()
            .filter_map(|game| game.game.uuid.as_deref())
            .filter_map(|uuid| Uuid::parse_str(uuid).ok())
            .collect::<std::collections::HashSet<_>>();
        let needs_adoption_scan = catalog.games.iter().any(|remote| {
            !local_uuids.contains(&remote.metadata.uuid)
                || initial_games
                    .iter()
                    .find(|local| {
                        local.game.uuid.as_deref() == Some(&remote.metadata.uuid.to_string())
                    })
                    .is_some_and(|local| {
                        self.remote_relation(&catalog, remote, local)
                            == crate::sync::Divergence::FastForward
                    })
        });
        let installed = if needs_adoption_scan {
            crate::install_scanner::scan_installed_candidates(
                &self.data_dir.join("cache"),
                &self.scan_roots_repo.get_scan_roots()?,
            )?
        } else {
            Vec::new()
        };
        let installed_titles = installed
            .iter()
            .map(|item| crate::repository::normalize_display_name(&item.title))
            .collect::<std::collections::HashSet<_>>();
        let installed_by_title = installed
            .iter()
            .map(|item| (crate::repository::normalize_display_name(&item.title), item))
            .collect::<std::collections::HashMap<_, _>>();
        let mut auto_applied = std::collections::HashSet::new();

        for remote_game in &catalog.games {
            let adopted = local_uuids.contains(&remote_game.metadata.uuid);
            let installed = installed_titles.contains(&crate::repository::normalize_display_name(
                &remote_game.metadata.title,
            ));
            if !adopted && !installed {
                continue;
            }
            let mut apply = !adopted
                && !crate::sync::remote_heads_diverge(
                    &catalog,
                    device.device_uuid,
                    remote_game.metadata.uuid,
                );
            if adopted
                && let Some(local) = initial_games.iter().find(|game| {
                    game.game.uuid.as_deref() == Some(&remote_game.metadata.uuid.to_string())
                })
                && self.remote_relation(&catalog, remote_game, local)
                    == crate::sync::Divergence::FastForward
            {
                let checkpoint = crate::checkpoint::checkpoint_current_state(
                    &self.working_base,
                    self.versions_repo.as_ref(),
                    local,
                    VersionReason::Watcher,
                )?;
                let unchanged = matches!(
                    checkpoint,
                    crate::checkpoint::CheckpointOutcome::Unchanged(_)
                );
                let install_path = installed_by_title
                    .get(&crate::repository::normalize_display_name(
                        &remote_game.metadata.title,
                    ))
                    .and_then(|item| item.install_path.as_deref());
                apply = crate::sync::decide_apply(
                    true,
                    crate::sync::Divergence::FastForward,
                    !unchanged,
                    crate::sync::game_is_running(install_path),
                ) == crate::sync::ApplyDecision::AutoApply;
            }
            apply = self.import_remote_game(&remote_root, &catalog, remote_game, apply)?;
            if apply {
                auto_applied.insert(remote_game.metadata.uuid);
            }
            local_uuids.insert(remote_game.metadata.uuid);
        }

        let games = self.games_repo.get_games()?;
        self.cmd_tx.send(Command::ReregisterGames(games.clone()))?;
        let state = self.device_state(&games)?;
        let published = crate::sync::push(&self.working_base, &remote_root, &device, &state)?;
        *self
            .device_metadata
            .write()
            .map_err(|_| anyhow!("Device metadata lock poisoned"))? = published;
        crate::sync::save_device(&self.data_dir, &self.device_metadata())?;

        let catalog = crate::sync::load_catalog(&remote_root)?;
        self.sync_report(&catalog, &installed_titles, &games, &auto_applied)
    }

    pub fn push_sync(&self) -> Result<()> {
        let _guard = self
            .sync_lock
            .lock()
            .map_err(|_| anyhow!("Sync lock poisoned"))?;
        self.push_sync_unlocked()
    }

    fn push_sync_unlocked(&self) -> Result<()> {
        let settings = self.get_sync_settings()?;
        let remote_root = PathBuf::from(
            settings
                .remote_path
                .as_deref()
                .context("Sync remote is not configured")?,
        );
        let device = self.device_metadata();
        let games = self.games_repo.get_games()?;
        let state = self.device_state(&games)?;
        let published = crate::sync::push(&self.working_base, &remote_root, &device, &state)?;
        *self
            .device_metadata
            .write()
            .map_err(|_| anyhow!("Device metadata lock poisoned"))? = published;
        crate::sync::save_device(&self.data_dir, &self.device_metadata())
    }

    pub fn adopt_remote_game(&self, game_uuid: Uuid) -> Result<FullGame> {
        let settings = self.get_sync_settings()?;
        let remote_root = PathBuf::from(
            settings
                .remote_path
                .as_deref()
                .context("Sync remote is not configured")?,
        );
        let catalog = crate::sync::load_catalog(&remote_root)?;
        for marker in catalog.retired_versions.values() {
            crate::pruning::write_marker(&self.working_base, marker)?;
        }
        let remote_game = catalog
            .games
            .iter()
            .find(|game| game.metadata.uuid == game_uuid)
            .context("Remote game not found")?;
        self.import_remote_game(&remote_root, &catalog, remote_game, true)?;
        let game = self
            .games_repo
            .get_games()?
            .into_iter()
            .find(|game| game.game.uuid.as_deref() == Some(&game_uuid.to_string()))
            .context("Adopted game was not persisted")?;
        let all_games = self.games_repo.get_games()?;
        self.cmd_tx.send(Command::ReregisterGames(all_games))?;
        Ok(game)
    }

    pub fn remove_sync_device(&self, device_uuid: Uuid) -> Result<()> {
        if device_uuid == self.device_metadata().device_uuid {
            return Err(anyhow!("The current device cannot remove itself"));
        }
        let settings = self.get_sync_settings()?;
        let remote_root = PathBuf::from(
            settings
                .remote_path
                .as_deref()
                .context("Sync remote is not configured")?,
        );
        crate::sync::remove_remote_device(&remote_root, device_uuid)
    }

    fn import_remote_game(
        &self,
        remote_root: &Path,
        catalog: &crate::sync::RemoteCatalog,
        remote_game: &crate::sync::RemoteGame,
        apply: bool,
    ) -> Result<bool> {
        let new_game =
            !self.games_repo.get_games()?.iter().any(|game| {
                game.game.uuid.as_deref() == Some(&remote_game.metadata.uuid.to_string())
            });
        let existing_saves = new_game && !Self::remote_save_dirs_empty(remote_game)?;
        let tip = crate::sync::latest_remote_tip(
            catalog,
            self.device_metadata().device_uuid,
            remote_game.metadata.uuid,
        );
        let apply = apply
            && !existing_saves
            && tip
                .as_ref()
                .and_then(|tip| tip.version_uuid)
                .is_some_and(|uuid| !catalog.retired_versions.contains_key(&uuid));
        let pulled = crate::sync::pull_game(
            &self.working_base,
            remote_root,
            remote_game,
            &catalog.retired_versions,
        )?;
        let state = tip.as_ref().map(|tip| {
            let mut state = tip.game_state.clone();
            if !apply {
                for slot in state.slots.values_mut() {
                    slot.active_version_uuid = None;
                }
            }
            state
        });
        let imported = self
            .games_repo
            .import_remote_game(&pulled, if new_game || apply { state } else { None })?;
        if existing_saves {
            crate::checkpoint::checkpoint_current_state(
                &self.working_base,
                self.versions_repo.as_ref(),
                &imported,
                VersionReason::Initial,
            )?;
        }
        if apply
            && let Some(version_uuid) = tip.and_then(|tip| tip.version_uuid)
            && let Some(version_id) = self.find_version_id(&imported, version_uuid)?
        {
            self.restore_version(version_id)?;
            return Ok(true);
        }
        Ok(false)
    }

    fn remote_save_dirs_empty(game: &crate::sync::RemoteGame) -> Result<bool> {
        for tracked in game
            .tracked_dirs
            .iter()
            .filter(|dir| dir.retired_at.is_none())
        {
            let path = PathBuf::from(crate::path_placeholders::expand_path_placeholders(
                &tracked.path,
            ));
            match std::fs::symlink_metadata(&path) {
                Ok(metadata) if !metadata.file_type().is_dir() => return Ok(false),
                Ok(_) if std::fs::read_dir(&path)?.next().is_some() => return Ok(false),
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error).context("Failed to inspect local save directory"),
            }
        }
        Ok(true)
    }

    fn find_version_id(&self, game: &FullGame, version_uuid: Uuid) -> Result<Option<i32>> {
        for slot in &game.slots {
            if let Some(version) = self
                .games_repo
                .get_slot(slot.id)?
                .versions
                .into_iter()
                .find(|version| version.uuid.as_deref() == Some(&version_uuid.to_string()))
            {
                return Ok(Some(version.id));
            }
        }
        Ok(None)
    }

    fn sync_report(
        &self,
        catalog: &crate::sync::RemoteCatalog,
        installed_titles: &std::collections::HashSet<String>,
        games: &[FullGame],
        auto_applied: &std::collections::HashSet<Uuid>,
    ) -> Result<crate::sync::SyncReport> {
        let local_by_uuid = games
            .iter()
            .filter_map(|game| {
                Uuid::parse_str(game.game.uuid.as_deref()?)
                    .ok()
                    .map(|uuid| (uuid, game))
            })
            .collect::<std::collections::HashMap<_, _>>();
        let device = self.device_metadata();
        let mut statuses = Vec::new();
        for remote_game in &catalog.games {
            let Some(local) = local_by_uuid.get(&remote_game.metadata.uuid) else {
                continue;
            };
            let Some(tip) = crate::sync::latest_remote_tip(
                catalog,
                device.device_uuid,
                remote_game.metadata.uuid,
            ) else {
                continue;
            };
            let remote_version_id = tip
                .version_uuid
                .map(|uuid| self.find_version_id(local, uuid))
                .transpose()?
                .flatten();
            statuses.push(crate::sync::GameSyncStatus {
                game_uuid: remote_game.metadata.uuid,
                relation: self.remote_relation(catalog, remote_game, local),
                remote_device: Some(tip.device_name),
                remote_version_uuid: tip.version_uuid,
                remote_version_id,
                auto_applied: auto_applied.contains(&remote_game.metadata.uuid),
            });
        }
        let remote_games = catalog
            .games
            .iter()
            .map(|game| crate::sync::RemoteGameSummary {
                uuid: game.metadata.uuid,
                title: game.metadata.title.clone(),
                installed: installed_titles.contains(&crate::repository::normalize_display_name(
                    &game.metadata.title,
                )),
                adopted: local_by_uuid.contains_key(&game.metadata.uuid),
            })
            .collect();
        Ok(crate::sync::SyncReport {
            synced_at: chrono::Utc::now(),
            games: statuses,
            remote_games,
            devices: catalog.devices.clone(),
            remote_versions: crate::sync::remote_version_devices(catalog, device.device_uuid),
        })
    }

    fn remote_relation(
        &self,
        catalog: &crate::sync::RemoteCatalog,
        remote_game: &crate::sync::RemoteGame,
        local: &FullGame,
    ) -> crate::sync::Divergence {
        if crate::sync::remote_heads_diverge(
            catalog,
            self.device_metadata().device_uuid,
            remote_game.metadata.uuid,
        ) {
            return crate::sync::Divergence::Diverged;
        }
        let remote_tip = crate::sync::latest_remote_tip(
            catalog,
            self.device_metadata().device_uuid,
            remote_game.metadata.uuid,
        )
        .and_then(|tip| tip.version_uuid);
        let local_tip = local
            .active_slot
            .as_ref()
            .and_then(|slot| {
                local
                    .versions
                    .iter()
                    .find(|version| Some(version.id) == slot.active_version_id)
            })
            .and_then(|version| version.uuid.as_deref())
            .and_then(|uuid| Uuid::parse_str(uuid).ok());
        let versions = remote_game
            .slots
            .iter()
            .flat_map(|slot| &slot.versions)
            .map(|version| (version.version.uuid, version.version.parent_version_uuid))
            .collect::<std::collections::HashMap<_, _>>();
        crate::sync::classify(local_tip, remote_tip, &versions)
    }

    fn device_state(&self, games: &[FullGame]) -> Result<crate::sync::DeviceState> {
        let slots = games
            .iter()
            .flat_map(|game| &game.slots)
            .map(|slot| {
                self.games_repo
                    .get_slot(slot.id)
                    .map(|full| (slot.id, full))
            })
            .collect::<Result<std::collections::HashMap<_, _>>>()?;
        crate::sync::DeviceState::from_games(self.device_metadata().device_uuid, games, &slots)
    }

    pub fn get_games(&self) -> Result<Vec<FullGame>> {
        self.games_repo.get_games()
    }

    pub fn get_game(&self, id: i32) -> Result<FullGame> {
        self.games_repo.get_game(id)
    }

    pub fn get_slots(&self, game_id: i32) -> Result<Vec<FullSlot>> {
        self.games_repo.list_slots(game_id)
    }

    pub fn save_game(&self, form: GameForm, dirs: Option<Vec<&str>>) -> Result<Game> {
        let game = self.games_repo.save_game(form, dirs)?;
        let all_games = self.games_repo.get_games()?;
        self.cmd_tx.send(Command::ReregisterGames(all_games))?;
        Ok(game)
    }

    pub fn delete_game(&self, id: i32) -> Result<()> {
        self.games_repo.delete_game(id)?;
        let all_games = self.games_repo.get_games()?;
        self.cmd_tx.send(Command::ReregisterGames(all_games))?;
        Ok(())
    }

    pub fn tag_version(&self, id: i32, tag: String) -> Result<Version> {
        crate::repository::with_exclusive_lock(&self.working_base, || {
            self.versions_repo.tag_version(id, tag)
        })
    }

    pub fn create_slot(&self, game_id: i32, name: &str) -> Result<FullGame> {
        self.run_with_paused_watcher(|| {
            let current_game = self
                .games_repo
                .get_game(game_id)
                .context("Failed to load game before slot creation")?;

            self.checkpoint_current_state(&current_game, VersionReason::PreSlotCreate)
                .context("Failed to checkpoint current slot before creating a new slot")?;

            let slot = self
                .games_repo
                .create_slot(game_id, name)
                .context("Failed to create slot")?;

            self.games_repo
                .set_active_slot(game_id, slot.slot.id)
                .context("Failed to activate new slot")?;

            let active_game = self
                .games_repo
                .get_game(game_id)
                .context("Failed to reload game after activating new slot")?;

            self.clear_tracked_dirs(&active_game)
                .context("Failed to clear tracked directories for new slot")?;

            crate::repository::with_exclusive_lock(&self.working_base, || {
                let empty_snapshot =
                    crate::content_store::capture_snapshot(&self.working_base, &[])
                        .context("Failed to capture empty baseline")?;
                self.versions_repo
                    .create_version_from_snapshot(game_id, VersionReason::Initial, empty_snapshot)
                    .context("Failed to create empty baseline for new slot")
            })?;

            self.resync_game_metadata(game_id)
                .context("Failed to sync metadata after creating slot")?;

            self.games_repo
                .get_game(game_id)
                .context("Failed to reload game after creating slot")
        })
    }

    pub fn switch_slot(&self, game_id: i32, slot_id: i32) -> Result<FullGame> {
        self.run_with_paused_watcher(|| {
            let current_game = self
                .games_repo
                .get_game(game_id)
                .context("Failed to load game before slot switch")?;

            if current_game
                .active_slot
                .as_ref()
                .is_some_and(|slot| slot.id == slot_id)
            {
                return Ok(current_game);
            }

            self.prepare_slot_activation(&current_game, slot_id)
                .context("Failed to validate target slot before switching")?;

            self.checkpoint_current_state(&current_game, VersionReason::PreSwitch)
                .context("Failed to checkpoint current slot before switching")?;

            crate::repository::with_exclusive_lock(&self.working_base, || {
                let prepared = self.prepare_slot_activation(&current_game, slot_id)?;
                self.activate_slot(&current_game, slot_id, prepared)
            })
        })
    }

    /// Makes `slot_id` the game's active slot and materialises its files into
    /// the live tracked directories. The caller owns the decision of whether
    /// the outgoing slot's live state is worth checkpointing first, and must
    /// have the watcher paused: this writes to tracked directories.
    fn prepare_slot_activation(
        &self,
        game: &FullGame,
        slot_id: i32,
    ) -> Result<PreparedSlotActivation> {
        let target_slot = game
            .slots
            .iter()
            .find(|slot| slot.id == slot_id)
            .context("Target slot not found before switching")?;

        if let Some(active_version_id) = target_slot.active_version_id {
            let full_version = self
                .versions_repo
                .get_version(active_version_id)
                .context("Failed to load active version for target slot")?;
            Ok(PreparedSlotActivation::Restore(
                self.prepare_restore_version_files(game, &full_version)
                    .context("Failed to validate target slot snapshot")?,
            ))
        } else {
            self.validate_tracked_dirs(game)
                .context("Failed to validate tracked directories for empty slot")?;
            Ok(PreparedSlotActivation::Clear)
        }
    }

    fn activate_slot(
        &self,
        game: &FullGame,
        slot_id: i32,
        prepared: PreparedSlotActivation,
    ) -> Result<FullGame> {
        match prepared {
            PreparedSlotActivation::Restore(restore) => self
                .apply_prepared_restore(restore)
                .context("Failed to restore target slot snapshot")?,
            PreparedSlotActivation::Clear => self
                .clear_tracked_dirs(game)
                .context("Failed to clear tracked directories for empty slot")?,
        }

        self.games_repo
            .set_active_slot(game.game.id, slot_id)
            .context("Failed to activate target slot")?;

        self.resync_game_metadata(game.game.id)
            .context("Failed to sync metadata after switching slots")?;

        self.games_repo
            .get_game(game.game.id)
            .context("Failed to reload game after switching slots")
    }

    pub fn rename_slot(&self, slot_id: i32, name: &str) -> Result<FullGame> {
        let slot = self
            .games_repo
            .rename_slot(slot_id, name)
            .context("Failed to rename slot")?;

        self.games_repo
            .get_game(slot.slot.game_id)
            .context("Failed to reload game after renaming slot")
    }

    /// Deletes a slot and the whole version history it owns.
    ///
    /// A game with no active slot has no coherent state, so two rules are
    /// enforced here rather than in the UI: the last remaining slot can never
    /// be deleted, and deleting the *active* slot requires naming the slot to
    /// switch to first. The replacement is switched into (restoring its files)
    /// before the target is dropped, so the live save directory always ends up
    /// holding some slot's real state.
    pub fn delete_slot(&self, slot_id: i32, replacement_slot_id: Option<i32>) -> Result<FullGame> {
        let slot = self
            .games_repo
            .get_slot(slot_id)
            .context("Failed to load slot for deletion")?;
        let game_id = slot.slot.game_id;
        let game = self
            .games_repo
            .get_game(game_id)
            .context("Failed to load game before slot deletion")?;

        if game.slots.len() <= 1 {
            return Err(anyhow!(
                "Cannot delete '{}': a game must keep at least one slot",
                slot.slot.name
            ));
        }

        let is_active = game
            .active_slot
            .as_ref()
            .is_some_and(|active| active.id == slot_id);

        if is_active {
            let replacement_slot_id = replacement_slot_id.context(
                "Deleting the active slot requires a replacement slot to switch to first",
            )?;
            if replacement_slot_id == slot_id {
                return Err(anyhow!(
                    "The replacement slot must not be the slot being deleted"
                ));
            }
            if !game
                .slots
                .iter()
                .any(|candidate| candidate.id == replacement_slot_id)
            {
                return Err(anyhow!(
                    "Replacement slot {replacement_slot_id} does not belong to this game"
                ));
            }

            // Deliberately no checkpoint of the outgoing slot: it is about to be
            // discarded, so capturing its live state would only mint a version
            // that the delete below immediately removes.
            self.prepare_slot_activation(&game, replacement_slot_id)
                .context("Failed to validate replacement slot before deleting")?;
            self.run_with_paused_watcher(|| {
                crate::repository::with_exclusive_lock(&self.working_base, || {
                    let prepared = self.prepare_slot_activation(&game, replacement_slot_id)?;
                    self.activate_slot(&game, replacement_slot_id, prepared)
                })
            })
            .context("Failed to switch to the replacement slot before deleting")?;
        }

        // The slot's directory under the repository is deliberately left on
        // disk, matching `delete_game`. Nothing in the app can bring a deleted
        // timeline back, so keeping the bytes is the reversible failure mode;
        // orphaned slot directories accumulate until a repository GC pass
        // reclaims them.
        self.games_repo
            .delete_slot(slot_id)
            .context("Failed to delete slot")?;

        self.games_repo
            .get_game(game_id)
            .context("Failed to reload game after deleting slot")
    }

    pub fn restore_version(&self, version_id: i32) -> Result<()> {
        self.run_with_paused_watcher(|| {
            crate::repository::with_exclusive_lock(&self.working_base, || {
                self.restore_version_unpaused(version_id)
            })
        })
    }

    pub fn apply_remote_version(&self, version_id: i32) -> Result<()> {
        self.run_with_paused_watcher(|| {
            let version = self.versions_repo.get_version(version_id)?;
            let game = self.games_repo.get_game(version.game.id)?;
            if game.active_slot.is_none() {
                return Err(anyhow!(
                    "Cannot apply remote progress without an active local slot"
                ));
            }
            crate::checkpoint::checkpoint_current_state(
                &self.working_base,
                self.versions_repo.as_ref(),
                &game,
                VersionReason::PreRestore,
            )?;
            crate::repository::with_exclusive_lock(&self.working_base, || {
                self.restore_version_unpaused(version_id)
            })
        })
    }

    fn restore_version_unpaused(&self, version_id: i32) -> Result<()> {
        // Read and validate BEFORE any filesystem mutation.
        let version = self
            .versions_repo
            .get_version(version_id)
            .context("Failed to load version for restoration")?;
        let game = self
            .games_repo
            .get_game(version.game.id)
            .context("Failed to reload game for restoration")?;

        if !game
            .slots
            .iter()
            .any(|slot| slot.id == version.version.slot_id)
        {
            return Err(anyhow!(
                "Version {} does not belong to game {}",
                version_id,
                game.game.id
            ));
        }

        let prepared = self
            .prepare_restore_version_files(&game, &version)
            .context("Failed to validate version files")?;
        self.apply_prepared_restore(prepared)
            .context("Failed to restore version files")?;
        self.versions_repo
            .set_active_version(version_id)
            .context("Failed to activate version for restoration")?;
        self.resync_game_metadata(game.game.id)
            .context("Failed to sync metadata after restoration")?;

        Ok(())
    }

    pub fn set_game_icon(&self, game_id: i32, source_path: &str) -> Result<String> {
        std::fs::create_dir_all(&self.icons_dir).context("Failed to create icons directory")?;

        let source = std::path::Path::new(source_path);
        let ext = source
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        let dest_path = self.icons_dir.join(format!("{}.png", game_id));

        if ext == "exe" {
            #[cfg(target_os = "windows")]
            extract_exe_icon(source, &dest_path)
                .context("Failed to extract icon from executable")?;

            #[cfg(not(target_os = "windows"))]
            anyhow::bail!("Executable icon extraction is only supported on Windows");
        } else {
            let img = image::open(source).context("Failed to open image file")?;
            img.save(&dest_path).context("Failed to save icon as PNG")?;
        }

        let icon_path = dest_path.to_string_lossy().into_owned();
        let game = self.games_repo.get_game(game_id)?;
        self.games_repo.save_game(
            GameForm {
                id: Some(game_id),
                uuid: game.game.uuid.clone(),
                title: &game.game.title,
                title_normalized: game.game.title_normalized.clone(),
                folder_slug: game.game.folder_slug.clone(),
                active_slot_id: game.game.active_slot_id,
                icon_path: Some(icon_path.clone()),
            },
            None,
        )?;

        Ok(icon_path)
    }

    pub fn get_scan_roots(&self) -> Result<Vec<String>> {
        self.scan_roots_repo.get_scan_roots()
    }

    pub fn add_scan_root(&self, path: &str) -> Result<()> {
        self.scan_roots_repo.add_scan_root(path)
    }

    pub fn remove_scan_root(&self, path: &str) -> Result<()> {
        self.scan_roots_repo.remove_scan_root(path)
    }

    /// Stops the watcher thread and everything it owns (the debouncer and its
    /// notification threads). Nothing else ever ends a watcher's life, so a
    /// process that builds several cores accumulates live debouncers, each one
    /// ticking and delivering events for directories nobody is looking at.
    pub fn shutdown(&self) {
        let _ = self.cmd_tx.send(Command::Shutdown);
    }

    pub fn get_monitoring_settings(&self) -> Result<MonitoringSettings> {
        load_monitoring_settings(&self.monitoring_settings_path)
    }

    pub fn update_monitoring_settings(
        &self,
        monitoring_settings: MonitoringSettings,
    ) -> Result<MonitoringSettings> {
        save_monitoring_settings(&self.monitoring_settings_path, monitoring_settings)?;
        self.cmd_tx
            .send(Command::UpdateMonitoringSettings(monitoring_settings))
            .context("Failed to update monitoring settings in watcher")?;
        Ok(monitoring_settings)
    }

    pub fn scan_installed(&self) -> Result<Vec<InstallScannerItem>> {
        let cache_dir = self
            .working_base
            .parent()
            .unwrap_or(&self.working_base)
            .join("cache");
        let custom_roots = self.scan_roots_repo.get_scan_roots()?;
        let mut items = crate::install_scanner::scan_installed(&cache_dir, &custom_roots)?;

        let icon_cache_dir = cache_dir.join("icon_cache");
        let _ = std::fs::create_dir_all(&icon_cache_dir);

        for item in &mut items {
            let Some(source_path) = &item.icon_path else {
                continue;
            };

            let safe_title: String = item
                .title
                .chars()
                .map(|c| {
                    if c.is_alphanumeric() || c == '-' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect();
            let cache_png = icon_cache_dir.join(format!("{safe_title}.png"));

            if cache_png.exists() {
                item.icon_path = Some(cache_png.to_string_lossy().into_owned());
                continue;
            }

            let source = std::path::Path::new(source_path);
            let ext = source
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();

            let extracted = if ext == "exe" {
                #[cfg(target_os = "windows")]
                {
                    extract_exe_icon(source, &cache_png).is_ok()
                }
                #[cfg(not(target_os = "windows"))]
                {
                    false
                }
            } else {
                image::open(source)
                    .and_then(|img| img.save(&cache_png))
                    .is_ok()
            };

            item.icon_path = if extracted {
                Some(cache_png.to_string_lossy().into_owned())
            } else {
                None
            };
        }

        Ok(items)
    }

    /// Runs `operation` with the watcher suppressed, so the writes it makes to
    /// tracked directories are not mistaken for the user playing the game.
    ///
    /// The contract is: pause and wait for the watcher to confirm it, mutate, let
    /// trailing OS events arrive while still paused, resume.
    /// Every wait is bounded and the watcher is always resumed — including when
    /// `operation` returns `Err` or panics, since leaving suppression latched
    /// would silently stop versioning for the rest of the session. A watcher that
    /// is gone or wedged costs suppression, never the operation itself.
    fn run_with_paused_watcher<T>(&self, operation: impl FnOnce() -> Result<T>) -> Result<T> {
        let (ack_tx, ack_rx) = std::sync::mpsc::channel();
        // A stopped watcher must not make restoring impossible. Propagating here
        // would fail every restore and slot switch for the rest of the session;
        // a spurious version is the better failure by a wide margin, so this logs
        // and falls through. The wait below then fails immediately (the ack sender
        // went with the dropped command) and the operation runs unsuppressed.
        if let Err(err) = self.cmd_tx.send(Command::Pause(ack_tx)) {
            eprintln!("Watcher is not accepting commands ({err}); continuing unsuppressed");
        }

        // Block until the watcher confirms it has seen the pause. Without this the
        // operation races an event the debouncer has already promoted to the queue
        // but the watcher has not dequeued yet: that event is still judged unpaused
        // and becomes a version. Rebuilding the debouncer on `Resume` cannot help
        // there — it only discards what is still buffered inside the debouncer.
        // Bounded: a watcher thread that died drops the ack sender and this returns
        // at once, and one that is wedged costs a bounded wait instead of hanging
        // the UI forever. Either way suppression degrades rather than deadlocks.
        if let Err(err) = ack_rx.recv_timeout(WATCHER_PAUSE_ACK_TIMEOUT) {
            // Distinguishes a wedged watcher (timed out) from a stopped one
            // (sending half closed), which are very different things to debug.
            eprintln!("Watcher did not acknowledge pause ({err}); continuing unsuppressed");
        }

        // `catch_unwind` so a panicking operation still reaches the resume below.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation));

        // Let trailing OS events for our own writes arrive while still paused, so
        // they are discarded instead of being delivered once watching resumes.
        std::thread::sleep(WATCHER_SETTLE_DELAY);

        let reregister_result = self
            .games_repo
            .get_games()
            .context("Failed to reload games after mutation")
            .and_then(|all_games| {
                self.cmd_tx
                    .send(Command::ReregisterGames(all_games))
                    .context("Failed to reregister games")
            });
        let resume_result = self
            .cmd_tx
            .send(Command::Resume)
            .context("Failed to send resume command");

        match result {
            Ok(Ok(value)) => {
                reregister_result?;
                resume_result?;
                Ok(value)
            }
            Ok(Err(err)) => {
                let _ = reregister_result;
                let _ = resume_result;
                Err(err)
            }
            // The watcher has been resumed; let the panic carry on unchanged.
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }

    fn checkpoint_current_state(
        &self,
        game: &FullGame,
        reason: VersionReason,
    ) -> Result<Option<FullVersion>> {
        if game.tracked_dirs.is_empty() {
            return Ok(None);
        }

        let outcome = crate::checkpoint::checkpoint_current_state(
            &self.working_base,
            self.versions_repo.as_ref(),
            game,
            reason,
        )?;
        let version = match outcome {
            crate::checkpoint::CheckpointOutcome::Unchanged(version)
            | crate::checkpoint::CheckpointOutcome::Created(version) => version,
        };
        let _ = self.push_sync();
        Ok(Some(version))
    }

    fn clear_tracked_dirs(&self, game: &FullGame) -> Result<()> {
        self.validate_tracked_dirs(game)?;

        for dir in &game.tracked_dirs {
            let live_dir = Path::new(&dir.path);
            if live_dir.exists() {
                crate::fs_utils::clear_dir_except(
                    live_dir,
                    &crate::exclusions::parse(&dir.exclusions),
                )
                .with_context(|| format!("Failed to clear tracked dir {}", live_dir.display()))?;
            }

            std::fs::create_dir_all(live_dir).with_context(|| {
                format!("Failed to recreate tracked dir {}", live_dir.display())
            })?;
        }

        Ok(())
    }

    fn validate_tracked_dirs(&self, game: &FullGame) -> Result<()> {
        for dir in &game.tracked_dirs {
            crate::fs_utils::validate_restore_path(Path::new(&dir.path), Path::new(""))
                .with_context(|| format!("Unsafe tracked dir {}", dir.path))?;
        }
        Ok(())
    }

    fn prepare_restore_version_files(
        &self,
        game: &FullGame,
        version: &FullVersion,
    ) -> Result<PreparedRestore> {
        if let Some(uuid) = version
            .version
            .uuid
            .as_deref()
            .and_then(|uuid| Uuid::parse_str(uuid).ok())
            && crate::pruning::read_markers(&self.working_base)?.contains_key(&uuid)
        {
            return Err(anyhow!(
                "This checkpoint was retired and can no longer be restored"
            ));
        }
        let game_folder_slug = game
            .game
            .folder_slug
            .as_deref()
            .context("Game is missing a folder slug")?;
        let version_folder_slug = version
            .version
            .folder_slug
            .as_deref()
            .context("Version is missing a folder slug")?;
        let version_dir = crate::repository::version_dir(
            &crate::repository::slot_dir(
                &crate::repository::game_dir(&self.working_base, game_folder_slug),
                &version.slot.folder_slug,
            ),
            version_folder_slug,
        );
        let metadata: crate::repository::VersionMetadata =
            crate::repository::read_metadata_json(&version_dir)
                .context("Failed to read version metadata")?;
        let game_uuid = uuid::Uuid::parse_str(
            game.game
                .uuid
                .as_deref()
                .context("Game is missing a UUID")?,
        )?;
        let mut roots = std::collections::HashMap::new();
        let mut restore_roots = std::collections::BTreeMap::new();
        for dir in &game.tracked_dirs {
            let uuid = uuid::Uuid::parse_str(
                dir.uuid
                    .as_deref()
                    .context("Tracked directory is missing a UUID")?,
            )?;
            let path = PathBuf::from(&dir.path);
            let exclusions = crate::exclusions::parse(&dir.exclusions);
            if let Some((previous, _)) = roots.insert(uuid, (path.clone(), exclusions.clone())) {
                anyhow::ensure!(
                    previous == path,
                    "Conflicting paths for tracked directory {uuid}"
                );
            }
            restore_roots.entry(path).or_insert(exclusions);
        }
        let tracked_root = crate::repository::tracked_dirs_dir(&crate::repository::game_dir(
            &self.working_base,
            game_folder_slug,
        ));
        if tracked_root.exists() {
            for entry in std::fs::read_dir(tracked_root)? {
                let entity_dir = entry?.path();
                if !entity_dir.is_dir() {
                    continue;
                }
                let dir: crate::repository::TrackedDirMetadata =
                    crate::repository::read_metadata_json(&entity_dir)?;
                anyhow::ensure!(
                    dir.game_uuid == game_uuid,
                    "Tracked directory metadata belongs to another game"
                );
                let path = PathBuf::from(crate::path_placeholders::expand_path_placeholders(
                    &dir.path,
                ));
                if let Some((previous, _)) = roots.get(&dir.uuid) {
                    anyhow::ensure!(
                        *previous == path,
                        "Conflicting paths for tracked directory {}",
                        dir.uuid
                    );
                } else {
                    let exclusions = dir
                        .exclusions
                        .iter()
                        .filter_map(|pattern| glob::Pattern::new(pattern).ok())
                        .collect();
                    roots.insert(dir.uuid, (path, exclusions));
                }
            }
        }
        for file in &metadata.files {
            let (path, exclusions) = roots.get(&file.tracked_dir_uuid).with_context(|| {
                format!(
                    "Version references unknown tracked directory {}",
                    file.tracked_dir_uuid
                )
            })?;
            restore_roots
                .entry(path.clone())
                .or_insert_with(|| exclusions.clone());
        }
        let validated_files =
            crate::content_store::validate_snapshot_blobs(&self.working_base, &metadata)
                .context("Failed to validate snapshot manifest")?;
        let mut files = Vec::new();
        for (file, blob) in validated_files {
            let (path, exclusions) = &roots[&file.tracked_dir_uuid];
            let relative = Path::new(&file.relative_path);
            if crate::exclusions::is_excluded(exclusions, relative) {
                continue;
            }
            files.push((path.clone(), relative.to_path_buf(), blob));
        }

        for path in restore_roots.keys() {
            crate::fs_utils::validate_restore_path(path, Path::new(""))
                .with_context(|| format!("Unsafe tracked dir {}", path.display()))?;
        }
        for (live_dir, relative, _) in &files {
            crate::fs_utils::validate_restore_path(live_dir, relative).with_context(|| {
                format!(
                    "Unsafe restore destination {}",
                    live_dir.join(relative).display()
                )
            })?;
        }

        Ok(PreparedRestore {
            restore_roots,
            files,
        })
    }

    fn apply_prepared_restore(&self, prepared: PreparedRestore) -> Result<()> {
        for (live_dir, exclusions) in &prepared.restore_roots {
            if live_dir.exists() {
                crate::fs_utils::clear_dir_except(live_dir, exclusions).with_context(|| {
                    format!("Failed to clear live tracked dir {}", live_dir.display())
                })?;
            }

            std::fs::create_dir_all(live_dir).with_context(|| {
                format!(
                    "Failed to recreate empty tracked dir {}",
                    live_dir.display()
                )
            })?;
        }

        for (live_dir, relative, blob) in prepared.files {
            let destination = live_dir.join(relative);
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("Failed to create restore directory {}", parent.display())
                })?;
            }
            std::fs::copy(&blob, &destination).with_context(|| {
                format!(
                    "Failed to restore blob {} to {}",
                    blob.display(),
                    destination.display()
                )
            })?;
        }

        Ok(())
    }

    fn resync_game_metadata(&self, game_id: i32) -> Result<()> {
        let game = self
            .games_repo
            .get_game(game_id)
            .context("Failed to load game for metadata sync")?;
        self.games_repo
            .save_game(
                GameForm {
                    id: Some(game.game.id),
                    uuid: game.game.uuid.clone(),
                    title: &game.game.title,
                    title_normalized: game.game.title_normalized.clone(),
                    folder_slug: game.game.folder_slug.clone(),
                    active_slot_id: game.game.active_slot_id,
                    icon_path: game.game.icon_path.clone(),
                },
                None,
            )
            .context("Failed to persist metadata sync")?;

        Ok(())
    }
}

fn load_monitoring_settings(path: &std::path::Path) -> Result<MonitoringSettings> {
    if !path.exists() {
        let settings = MonitoringSettings::default();
        save_monitoring_settings(path, settings)?;
        return Ok(settings);
    }

    let contents = fs::read_to_string(path).context("Failed to read monitoring settings")?;
    let settings =
        serde_json::from_str(&contents).context("Failed to parse monitoring settings")?;
    Ok(settings)
}

fn save_monitoring_settings(
    path: &std::path::Path,
    monitoring_settings: MonitoringSettings,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("Failed to create monitoring settings directory")?;
    }

    let contents = serde_json::to_string_pretty(&monitoring_settings)
        .context("Failed to serialize monitoring settings")?;
    fs::write(path, contents).context("Failed to write monitoring settings")?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn extract_exe_icon(
    exe_path: &std::path::Path,
    output_path: &std::path::Path,
) -> anyhow::Result<()> {
    let script = "Add-Type -AssemblyName System.Drawing; \
                  $icon = [System.Drawing.Icon]::ExtractAssociatedIcon($env:EXE_PATH); \
                  $icon.ToBitmap().Save($env:OUT_PATH)";
    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("EXE_PATH", exe_path)
        .env("OUT_PATH", output_path)
        .status()
        .context("Failed to run PowerShell")?;
    anyhow::ensure!(status.success(), "PowerShell icon extraction failed");
    Ok(())
}
