//! The local history store: the only writer of local history. Every mutation
//! writes repository files first and then projects them into the index, so a
//! crash leaves at worst a stale index that `reindex` repairs (ADR 0001).

use crate::chunks::{self, CaptureRoot};
use crate::index::Index;
use crate::models::*;
use crate::store::{
    FileEntry, GameMeta, GameRecord, SlotMeta, Store, TrackedDirMeta, VersionMeta, VersionReason,
    folder_slug, normalize_display_name,
};
use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Utc};
use std::collections::HashSet;
use std::path::Path;
use uuid::Uuid;

pub const DEFAULT_SLOT_NAME: &str = "Default Slot";

#[derive(Debug, Clone, PartialEq)]
pub enum CheckpointOutcome {
    Unchanged(Version),
    Created(Version),
    /// Nothing to record: no saves and no history yet.
    Empty,
}

impl CheckpointOutcome {
    pub fn version(&self) -> Option<&Version> {
        match self {
            Self::Unchanged(version) | Self::Created(version) => Some(version),
            Self::Empty => None,
        }
    }
}

pub struct History {
    pub store: Store,
    pub index: Index,
    device: Uuid,
}

impl History {
    pub fn open(root: &Path, index: Index, device: Uuid) -> Result<Self> {
        let history = Self {
            store: Store::open(root)?,
            index,
            device,
        };
        // Files are the truth: a crash between publishing files and projecting
        // them leaves a stale index, so every start re-projects.
        history.reindex()?;
        Ok(history)
    }

    pub fn root(&self) -> &Path {
        self.store.root()
    }

    pub fn retired(&self) -> Result<HashSet<Uuid>> {
        Ok(self.store.markers()?.into_keys().collect())
    }

    /// Rebuilds the whole index from the repository files.
    pub fn reindex(&self) -> Result<()> {
        self.index.reindex(&self.store.catalog()?, &self.retired()?)
    }

    /// Re-projects one game from its files into the index.
    pub fn refresh_game(&self, uuid: Uuid) -> Result<()> {
        match self.store.game(uuid)? {
            Some(record) => self.index.upsert_game(&record, &self.retired()?),
            None => self.index.remove_game(uuid),
        }
    }

    pub fn record(&self, game: &Game) -> Result<GameRecord> {
        self.store
            .game_by_slug(&game.folder_slug)?
            .with_context(|| format!("files for game '{}' are missing", game.title))
    }

    pub fn version_meta(&self, version: &FullVersion) -> Result<VersionMeta> {
        self.store.version_meta(
            &version.game.folder_slug,
            &version.slot.folder_slug,
            &version.version.folder_slug,
        )
    }

    pub fn create_game(&self, title: &str, dirs: &[String]) -> Result<FullGame> {
        let title = self.validate_title(title, None)?;
        let now = Utc::now();
        let uuid = Uuid::new_v4();
        let meta = GameMeta {
            uuid,
            title: title.clone(),
            folder_slug: folder_slug('g', &title, uuid),
            created_at: now,
            updated_at: now,
            deleted_at: None,
        };
        self.validate_dirs(None, dirs)?;
        self.store.write_game(&meta)?;
        self.write_slot_meta(&meta.folder_slug, uuid, DEFAULT_SLOT_NAME)?;
        self.write_dirs(&meta, &[], dirs)?;
        self.refresh_game(uuid)?;
        let id = self
            .index
            .game_id(uuid)?
            .context("created game is not indexed")?;
        self.index.get_game(id)
    }

    /// Renames a game and, when `dirs` is given, sets its tracked directories:
    /// missing ones are retired, known ones reactivated, new ones added.
    pub fn update_game(&self, id: i32, title: &str, dirs: Option<&[String]>) -> Result<FullGame> {
        let game = self.index.get_game(id)?;
        let record = self.record(&game.game)?;
        let title = self.validate_title(title, Some(id))?;
        if let Some(dirs) = dirs {
            self.validate_dirs(Some(id), dirs)?;
        }
        if title != record.meta.title {
            self.store.write_game(&GameMeta {
                title,
                updated_at: Utc::now(),
                ..record.meta.clone()
            })?;
        }
        if let Some(dirs) = dirs {
            self.write_dirs(&record.meta, &record.tracked_dirs, dirs)?;
        }
        self.refresh_game(record.meta.uuid)?;
        self.index.get_game(id)
    }

    pub fn delete_game(&self, id: i32) -> Result<()> {
        let game = self.index.get_game(id)?;
        let record = self.record(&game.game)?;
        let now = Utc::now();
        self.store.write_game(&GameMeta {
            deleted_at: Some(now),
            updated_at: now,
            ..record.meta
        })?;
        self.refresh_game(record.meta.uuid)
    }

    pub fn create_slot(&self, game_id: i32, name: &str) -> Result<Slot> {
        let game = self.index.get_game(game_id)?;
        let name = self.validate_slot_name(&game, name, None)?;
        let uuid = self.write_slot_meta(&game.game.folder_slug, game_uuid(&game.game)?, &name)?;
        self.refresh_game(game_uuid(&game.game)?)?;
        let slot_id = self
            .index
            .slot_id(uuid)?
            .context("created slot is not indexed")?;
        Ok(self.index.get_slot(slot_id)?.slot)
    }

    pub fn rename_slot(&self, slot_id: i32, name: &str) -> Result<Slot> {
        let slot = self.index.get_slot(slot_id)?.slot;
        let game = self.index.get_game(slot.game_id)?;
        let name = self.validate_slot_name(&game, name, Some(slot_id))?;
        let mut meta = self.slot_meta(&game, &slot)?;
        meta.name = name;
        meta.updated_at = Utc::now();
        self.store.write_slot(&game.game.folder_slug, &meta)?;
        self.refresh_game(game_uuid(&game.game)?)?;
        Ok(self.index.get_slot(slot_id)?.slot)
    }

    /// Tombstones a slot so no device resurrects it. Its versions become
    /// unreachable and are collected by maintenance.
    pub fn delete_slot(&self, slot_id: i32) -> Result<()> {
        let slot = self.index.get_slot(slot_id)?.slot;
        let game = self.index.get_game(slot.game_id)?;
        let record = self
            .record(&game.game)?
            .slots
            .into_iter()
            .find(|record| record.meta.uuid.to_string() == slot.uuid)
            .with_context(|| format!("files for slot '{}' are missing", slot.name))?;
        let mut meta = record.meta;
        let now = Utc::now();
        meta.deleted_at = Some(now);
        meta.seen_versions = record.versions.iter().map(|version| version.uuid).collect();
        meta.updated_at = now;
        self.store.write_slot(&game.game.folder_slug, &meta)?;
        self.refresh_game(game_uuid(&game.game)?)
    }

    pub fn tag_version(&self, id: i32, tag: Option<String>) -> Result<Version> {
        let version = self.index.get_version(id)?;
        let mut meta = self.version_meta(&version)?;
        meta.tag = tag
            .map(|tag| tag.trim().to_string())
            .filter(|tag| !tag.is_empty());
        meta.updated_at = Utc::now().max(meta.updated_at + chrono::Duration::milliseconds(1));
        self.store
            .publish_version(&version.game.folder_slug, &version.slot.folder_slug, &meta)?;
        self.refresh_game(game_uuid(&version.game)?)?;
        Ok(self.index.get_version(id)?.version)
    }

    /// Captures the live tracked directories and records a new version on the
    /// active slot unless the content equals the active version. The caller
    /// must hold the repository lock.
    pub fn checkpoint_locked(
        &self,
        game: &FullGame,
        reason: VersionReason,
    ) -> Result<CheckpointOutcome> {
        let slot = game
            .active_slot
            .as_ref()
            .context("game has no active slot")?;
        let files = self.capture(game)?;
        if game.last_version.is_none() && files.is_empty() {
            return Ok(CheckpointOutcome::Empty);
        }
        if let Some(active) = &game.last_version {
            let active_meta = self.store.version_meta(
                &game.game.folder_slug,
                &slot.folder_slug,
                &active.folder_slug,
            );
            if active_meta.is_ok_and(|meta| meta.files == files) {
                return Ok(CheckpointOutcome::Unchanged(active.clone()));
            }
        }
        let parent_uuid = game
            .last_version
            .as_ref()
            .map(|version| Uuid::parse_str(&version.uuid))
            .transpose()?;
        self.commit_version(game, slot, parent_uuid, reason, files, Utc::now())
            .map(CheckpointOutcome::Created)
    }

    /// The live contents of the tracked directories, stored as chunks. The
    /// caller must hold the repository lock.
    pub fn capture(&self, game: &FullGame) -> Result<Vec<FileEntry>> {
        let exclusions: Vec<_> = game
            .tracked_dirs
            .iter()
            .map(|dir| crate::exclusions::parse(&dir.exclusions))
            .collect();
        let roots = game
            .tracked_dirs
            .iter()
            .zip(&exclusions)
            .map(|(dir, exclusions)| {
                Ok(CaptureRoot {
                    uuid: Uuid::parse_str(&dir.uuid)?,
                    path: Path::new(&dir.path),
                    exclusions,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        chunks::capture(&self.store, &roots).context("failed to capture tracked directories")
    }

    /// Records `files`, captured at `created_at`, as a new version on `slot`
    /// and makes it the slot's active one. The caller must hold the
    /// repository lock.
    pub fn commit_version(
        &self,
        game: &FullGame,
        slot: &Slot,
        parent_uuid: Option<Uuid>,
        reason: VersionReason,
        files: Vec<FileEntry>,
        created_at: DateTime<Utc>,
    ) -> Result<Version> {
        let slot_record = self.index.get_slot(slot.id)?;
        let number = slot_record
            .versions
            .iter()
            .map(|v| v.version_number)
            .max()
            .unwrap_or(0)
            + 1;
        let uuid = Uuid::new_v4();
        let now = Utc::now();
        let meta = VersionMeta {
            uuid,
            game_uuid: game_uuid(&game.game)?,
            slot_uuid: Uuid::parse_str(&slot.uuid)?,
            parent_uuid,
            version_number: number,
            folder_slug: folder_slug('v', &format!("v{number:03}"), uuid),
            reason,
            tag: None,
            created_at,
            updated_at: now,
            origin_device: Some(self.device),
            files,
        };
        self.store
            .publish_version(&game.game.folder_slug, &slot.folder_slug, &meta)?;
        self.refresh_game(meta.game_uuid)?;
        let id = self
            .index
            .version_id(uuid)?
            .context("new version is not indexed")?;
        self.index.set_active_version(slot.id, Some(id))?;
        Ok(self.index.get_version(id)?.version)
    }

    fn slot_meta(&self, game: &FullGame, slot: &Slot) -> Result<SlotMeta> {
        self.record(&game.game)?
            .slots
            .into_iter()
            .map(|record| record.meta)
            .find(|meta| meta.uuid.to_string() == slot.uuid)
            .with_context(|| format!("files for slot '{}' are missing", slot.name))
    }

    fn write_slot_meta(&self, game_slug: &str, game_uuid: Uuid, name: &str) -> Result<Uuid> {
        let now = Utc::now();
        let uuid = Uuid::new_v4();
        self.store.write_slot(
            game_slug,
            &SlotMeta {
                uuid,
                game_uuid,
                folder_slug: folder_slug('s', name, uuid),
                name: name.to_string(),
                created_at: now,
                updated_at: now,
                deleted_at: None,
                seen_versions: Vec::new(),
            },
        )?;
        Ok(uuid)
    }

    fn write_dirs(
        &self,
        game: &GameMeta,
        existing: &[TrackedDirMeta],
        dirs: &[String],
    ) -> Result<()> {
        let now = Utc::now();
        let wanted: Vec<String> = dirs
            .iter()
            .map(|dir| crate::path_placeholders::substitute_path_placeholders(Path::new(dir)))
            .collect();
        for dir in existing {
            let keep = wanted.iter().any(|path| same_path(path, &dir.path));
            if keep == dir.retired_at.is_some() {
                self.store.write_tracked_dir(
                    &game.folder_slug,
                    &TrackedDirMeta {
                        retired_at: (!keep).then_some(now),
                        updated_at: now,
                        ..dir.clone()
                    },
                )?;
            }
        }
        for path in wanted {
            if existing.iter().any(|dir| same_path(&dir.path, &path)) {
                continue;
            }
            let uuid = Uuid::new_v4();
            let name = path
                .rsplit(['/', '\\'])
                .find(|part| !part.is_empty())
                .unwrap_or("dir");
            self.store.write_tracked_dir(
                &game.folder_slug,
                &TrackedDirMeta {
                    uuid,
                    game_uuid: game.uuid,
                    folder_slug: folder_slug('d', name, uuid),
                    path,
                    label: None,
                    exclusions: Vec::new(),
                    retired_at: None,
                    created_at: now,
                    updated_at: now,
                },
            )?;
        }
        Ok(())
    }

    fn validate_title(&self, title: &str, id: Option<i32>) -> Result<String> {
        let title = title.trim();
        if title.is_empty() {
            bail!("A game title cannot be empty");
        }
        let normalized = normalize_display_name(title);
        if self
            .index
            .get_games()?
            .iter()
            .any(|game| Some(game.game.id) != id && game.game.title_normalized == normalized)
        {
            bail!("A game named '{title}' already exists");
        }
        Ok(title.to_string())
    }

    fn validate_slot_name(&self, game: &FullGame, name: &str, id: Option<i32>) -> Result<String> {
        let name = name.trim();
        if name.is_empty() {
            bail!("A slot name cannot be empty");
        }
        let normalized = normalize_display_name(name);
        if game
            .slots
            .iter()
            .any(|slot| Some(slot.id) != id && slot.name_normalized == normalized)
        {
            bail!("A slot named '{name}' already exists for this game");
        }
        Ok(name.to_string())
    }

    /// Tracked directories must be absolute and not overlap another game's.
    fn validate_dirs(&self, id: Option<i32>, dirs: &[String]) -> Result<()> {
        let taken = self.index.tracked_paths()?;
        for dir in dirs {
            if !Path::new(dir).is_absolute() {
                bail!("Tracked directory must be an absolute path: {dir}");
            }
            if let Some((_, path)) = taken
                .iter()
                .find(|(game_id, path)| Some(*game_id) != id && overlapping_paths(path, dir))
            {
                bail!("{dir} overlaps {path}, which another game tracks");
            }
        }
        Ok(())
    }
}

pub fn game_uuid(game: &Game) -> Result<Uuid> {
    Uuid::parse_str(&game.uuid).map_err(|_| anyhow!("game {} has an invalid UUID", game.id))
}

/// Resolves `.` and `..` the way Windows does, by name, so aliases of one
/// folder compare equal.
fn normalize_path(path: &str) -> String {
    let path = path.replace('\\', "/");
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "." => {}
            "" if !parts.is_empty() => {}
            ".." if parts.len() > 1 => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    let path = parts.join("/");
    let path = path.trim_end_matches('/').to_string();
    if cfg!(windows) {
        path.to_lowercase()
    } else {
        path
    }
}

/// Path equality as the platform sees it (case-insensitive on Windows).
pub fn same_path(left: &str, right: &str) -> bool {
    normalize_path(left) == normalize_path(right)
}

/// Whether one path is the other or lies inside it. Two games must never
/// track overlapping folders: clearing one would wipe the other's saves.
pub fn overlapping_paths(left: &str, right: &str) -> bool {
    let (left, right) = (normalize_path(left), normalize_path(right));
    let inside = |inner: &str, outer: &str| {
        inner
            .strip_prefix(outer)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    };
    inside(&left, &right) || inside(&right, &left)
}
