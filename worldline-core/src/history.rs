//! The local history store: the only writer of local history. Every mutation
//! writes repository files first and then projects them into the index, so a
//! crash leaves at worst a stale index that `reindex` repairs (ADR 0001).

use crate::chunks::{self, CaptureRoot};
use crate::error::ErrorKind;
use crate::icon;
use crate::index::Index;
use crate::models::*;
use crate::store::{
    AccentHueSource, Deletion, FileEntry, GameMeta, GameRecord, IconSource, SlotMeta, Store,
    TrackedDirMeta, VersionMeta, VersionReason, folder_slug, normalize_display_name, read_json,
    validate_hue, write_json,
};
use crate::sync::{DeviceGameState, DeviceState};
use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub const DEFAULT_SLOT_NAME: &str = "Default Slot";

#[derive(Debug, Clone, PartialEq)]
pub enum CheckpointOutcome {
    Unchanged(Version),
    Created(Version),
    /// Nothing to record: no saves and no history yet.
    Empty,
}

/// What `WorldlineCore::keep` did.
#[derive(Debug)]
pub struct Kept {
    /// The capture. A created version stays in history even when tagging it then failed.
    pub checkpoint: CheckpointOutcome,
    /// The parent a created version branches off, as captures report it.
    pub branch_from: Option<crate::detection::BranchFrom>,
    /// The version with its tag, or why it couldn't be tagged; `None` when nothing was captured.
    pub tagged: Option<Result<Version>>,
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
    /// Device-local list of games this device stopped tracking (ADR 0006).
    untracked_path: PathBuf,
    /// Device-local pointers of deleted games, kept next to the untracked
    /// list while they can be recovered (ADR 0011).
    deleted_pointers_path: PathBuf,
}

impl History {
    pub fn open(root: &Path, index: Index, device: Uuid, untracked_path: &Path) -> Result<Self> {
        let history = Self {
            store: Store::open(root)?,
            index,
            device,
            untracked_path: untracked_path.to_path_buf(),
            deleted_pointers_path: untracked_path.with_file_name("deleted_pointers.json"),
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

    /// Rebuilds the whole index from the repository files. Untracked games
    /// keep their files but stay out of the index.
    pub fn reindex(&self) -> Result<()> {
        let untracked = self.untracked()?;
        let mut catalog = self.store.catalog()?;
        catalog.retain(|game| !untracked.contains(&game.meta.uuid));
        self.index.reindex(&catalog, &self.retired()?, &self.store)
    }

    /// Re-projects one game from its files into the index.
    pub fn refresh_game(&self, uuid: Uuid) -> Result<()> {
        if self.untracked()?.contains(&uuid) {
            return self.index.remove_game(uuid);
        }
        match self.store.game(uuid)? {
            Some(record) => self
                .index
                .upsert_game(&record, &self.retired()?, &self.store),
            None => self.index.remove_game(uuid),
        }
    }

    /// The pointers kept for deleted games. They only help a recovery resume,
    /// so a damaged file is set aside and starts over empty.
    fn deleted_pointers(&self) -> Result<HashMap<Uuid, DeviceGameState>> {
        let path = &self.deleted_pointers_path;
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(HashMap::new());
            }
            Err(error) => {
                return Err(error).context("Failed to load the pointers of deleted games");
            }
        };
        match serde_json::from_slice(&bytes) {
            Ok(kept) => Ok(kept),
            Err(_) => {
                let aside = path.with_file_name(format!(
                    ".deleted_pointers.corrupt-{}.json",
                    Uuid::new_v4().simple()
                ));
                // The next write replaces the file anyway.
                let _ = std::fs::rename(path, aside);
                Ok(HashMap::new())
            }
        }
    }

    /// Records this device's pointers of a game about to be deleted, before
    /// the deletion is written, and forgets those of games that can no longer
    /// be recovered. One whose files can't be read now is kept.
    pub(crate) fn keep_pointers(&self, uuid: Uuid) -> Result<()> {
        let pointers: Vec<_> = self
            .index
            .pointers()?
            .into_iter()
            .filter(|pointer| pointer.game == uuid)
            .collect();
        let mut kept = self.deleted_pointers()?;
        let now = Utc::now();
        kept.retain(|game, _| match self.store.find_game(*game) {
            Ok(meta) => meta.is_some_and(|meta| meta.deletion.is_recoverable(now)),
            Err(_) => true,
        });
        match DeviceState::from_pointers(self.device, &pointers)
            .games
            .remove(&uuid)
        {
            Some(state) => kept.insert(uuid, state),
            None => kept.remove(&uuid),
        };
        write_json(&self.deleted_pointers_path, &kept)
    }

    /// Games this device stopped tracking; their history stays in the store.
    pub fn untracked(&self) -> Result<HashSet<Uuid>> {
        if !self.untracked_path.exists() {
            return Ok(HashSet::new());
        }
        read_json(&self.untracked_path).context("Failed to load the untracked games")
    }

    /// Records whether this device tracks `uuid`, without touching the index:
    /// callers re-project the game when they are ready for it.
    pub fn set_untracked(&self, uuid: Uuid, untracked: bool) -> Result<()> {
        let mut games = self.untracked()?;
        let changed = if untracked {
            games.insert(uuid)
        } else {
            games.remove(&uuid)
        };
        if changed {
            write_json(&self.untracked_path, &games)?;
        }
        Ok(())
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

    pub fn write_screenshot(&self, version_id: i32, jpeg: &[u8]) -> Result<()> {
        let version = self.index.get_version(version_id)?;
        self.store.write_screenshot(
            &version.game.folder_slug,
            &version.slot.folder_slug,
            &version.version.folder_slug,
            jpeg,
        )
    }

    /// The version's screenshot, if it has one; a retired version has none.
    pub fn screenshot_path(&self, version_id: i32) -> Result<Option<PathBuf>> {
        let version = self.index.get_version(version_id)?;
        let path = self.store.screenshot_path(
            &version.game.folder_slug,
            &version.slot.folder_slug,
            &version.version.folder_slug,
        );
        Ok((!version.version.retired && path.is_file()).then_some(path))
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
            deletion: Default::default(),
            icon_sha256: None,
            accent_hue: None,
            accent_hue_source: AccentHueSource::Auto,
            icon_source: IconSource::User,
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

    /// Stores `png` (already normalized) as the game's icon, chosen by
    /// `source`. An automatic accent hue follows the icon; a user-chosen one
    /// is kept. The caller holds the repository lock.
    pub fn set_icon(&self, id: i32, png: &[u8], source: IconSource) -> Result<FullGame> {
        let mut meta = self.game_meta(id)?;
        if meta.accent_hue_source == AccentHueSource::Auto {
            meta.accent_hue = icon::dominant_hue(&icon::decode(png)?);
        }
        meta.icon_sha256 = Some(icon::sha256(png));
        meta.icon_source = source;
        meta.updated_at = Utc::now().max(meta.updated_at + chrono::Duration::milliseconds(1));
        // The icon first: the metadata must never name an icon not yet written.
        self.store.write_icon(&meta.folder_slug, png)?;
        self.store.write_game(&meta)?;
        self.refresh_game(meta.uuid)?;
        self.index.get_game(id)
    }

    /// `Some` sets a user-chosen accent hue; `None` returns to the hue of the
    /// icon (or the UUID fallback when it has no usable color). The caller
    /// holds the repository lock.
    pub fn set_accent(&self, id: i32, hue: Option<u16>) -> Result<FullGame> {
        let mut meta = self.game_meta(id)?;
        match hue {
            Some(hue) => {
                validate_hue(hue)?;
                meta.accent_hue = Some(hue);
                meta.accent_hue_source = AccentHueSource::User;
            }
            None => {
                meta.accent_hue = self.icon_hue(&meta);
                meta.accent_hue_source = AccentHueSource::Auto;
            }
        }
        meta.updated_at = Utc::now().max(meta.updated_at + chrono::Duration::milliseconds(1));
        self.store.write_game(&meta)?;
        self.refresh_game(meta.uuid)?;
        self.index.get_game(id)
    }

    /// Whether an automatic icon of `size` may take the place of `meta`'s:
    /// it has none, or an automatic one (arrived) that `size` is sharper
    /// than. A user's icon, or one not arrived yet, is never replaced.
    pub fn takes_auto_icon(&self, meta: &GameMeta, size: (u32, u32)) -> bool {
        if meta.icon_sha256.is_none() {
            return true;
        }
        meta.icon_source == IconSource::Auto
            && self
                .icon_size(meta)
                .is_some_and(|current| icon::is_sharper(size, current))
    }

    /// The size of the stored icon `meta` names, if it has arrived.
    pub fn icon_size(&self, meta: &GameMeta) -> Option<(u32, u32)> {
        let png = self.store.read_icon(&meta.folder_slug).ok()??;
        if meta.icon_sha256.as_deref() != Some(icon::sha256(&png).as_str()) {
            return None;
        }
        icon::dimensions(&png).ok()
    }

    /// The dominant hue of the stored icon `meta` names, if it has arrived.
    fn icon_hue(&self, meta: &GameMeta) -> Option<u16> {
        let png = self.store.read_icon(&meta.folder_slug).ok()??;
        if meta.icon_sha256.as_deref() != Some(icon::sha256(&png).as_str()) {
            return None;
        }
        icon::dominant_hue(&icon::decode(&png).ok()?)
    }

    pub(crate) fn game_meta(&self, id: i32) -> Result<GameMeta> {
        let game = self.index.get_game(id)?.game;
        self.store
            .game_meta(&game.folder_slug)?
            .with_context(|| format!("files for game '{}' are missing", game.title))
    }

    pub fn delete_game(&self, id: i32) -> Result<()> {
        let game = self.index.get_game(id)?;
        let record = self.record(&game.game)?;
        let mut meta = record.meta;
        self.keep_pointers(meta.uuid)?;
        let now = Utc::now();
        meta.deletion.delete(now);
        meta.updated_at = now;
        self.store.write_game(&meta)?;
        self.refresh_game(meta.uuid)
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
        meta.deletion.delete(now);
        meta.seen_versions = record.versions.iter().map(|version| version.uuid).collect();
        meta.updated_at = now;
        self.store.write_slot(&game.game.folder_slug, &meta)?;
        self.refresh_game(game_uuid(&game.game)?)
    }

    /// Games and slots deleted everywhere that can still be recovered, newest
    /// first (ADR 0011). Slots of a deleted or untracked game are left out:
    /// they can only come back into a game this device shows.
    pub fn recently_deleted(&self, now: DateTime<Utc>) -> Result<Vec<DeletedItem>> {
        let untracked = self.untracked()?;
        let mut items = Vec::new();
        for game in self.store.game_metas()? {
            if let Some(expires_at) = recoverable_until(&game.deletion, now) {
                items.push(DeletedItem {
                    kind: DeletedKind::Game,
                    uuid: game.uuid,
                    title: game.title.clone(),
                    game_title: game.title.clone(),
                    deleted_at: game.deletion.deleted_at.unwrap_or_default(),
                    expires_at,
                });
            }
            if game.deletion.is_deleted() || untracked.contains(&game.uuid) {
                continue;
            }
            for slot in self.store.slot_metas(&game)? {
                if let Some(expires_at) = recoverable_until(&slot.deletion, now) {
                    items.push(DeletedItem {
                        kind: DeletedKind::Slot,
                        uuid: slot.uuid,
                        title: slot.name,
                        game_title: game.title.clone(),
                        deleted_at: slot.deletion.deleted_at.unwrap_or_default(),
                        expires_at,
                    });
                }
            }
        }
        items.sort_by_key(|item| std::cmp::Reverse(item.deleted_at));
        Ok(items)
    }

    /// Brings back a deleted game with the history it had, without writing
    /// live saves: the next checkpoint records whatever is on disk, on top of
    /// the version this device was on. Refused while another game here has
    /// its title or one of its folders, since both would capture the same
    /// saves. A game this device doesn't track comes back for the other
    /// devices and stays in the cloud list. Returns whether it is back in
    /// this device's library.
    pub fn recover_game(&self, uuid: Uuid, now: DateTime<Utc>) -> Result<bool> {
        let record = self
            .store
            .game(uuid)?
            .context("This game is no longer in your library")?;
        let mut meta = record.meta.clone();
        if !meta.deletion.is_recoverable(now) {
            bail!("'{}' can no longer be restored", meta.title);
        }
        let here = !self.untracked()?.contains(&uuid);
        if here {
            self.validate_fits(&record)?;
        }
        // Read before anything changes; the entry goes once the game can no
        // longer be recovered.
        let saved = self.deleted_pointers()?.remove(&uuid);
        meta.deletion.recover(now);
        meta.updated_at = now;
        self.store.write_game(&meta)?;
        self.refresh_game(uuid)?;
        if here {
            self.resume_pointers(uuid, saved)?;
        }
        Ok(here)
    }

    /// Refuses a game whose title or one of whose folders another game here
    /// already has: both would capture the same saves.
    fn validate_fits(&self, record: &GameRecord) -> Result<()> {
        self.validate_title(&record.meta.title, None)?;
        let dirs: Vec<String> = record
            .tracked_dirs
            .iter()
            .filter(|dir| dir.retired_at.is_none())
            .map(|dir| crate::path_placeholders::expand_path_placeholders(&dir.path))
            .filter(|path| !crate::path_placeholders::is_shared_root(Path::new(path)))
            .collect();
        self.validate_dirs(None, &dirs)
    }

    /// Points a game recovered here where this device was when it was
    /// deleted: the playthrough it was on and each playthrough's version.
    /// Without a record it resumes as an adoption does, from the newest
    /// version of its active playthrough.
    fn resume_pointers(&self, uuid: Uuid, saved: Option<DeviceGameState>) -> Result<()> {
        let game_id = self
            .index
            .game_id(uuid)?
            .context("Recovered game is not indexed")?;
        let retired = self.retired()?;
        let Some(state) = saved else {
            if let Some(slot) = self.index.get_game(game_id)?.active_slot {
                let newest = self
                    .index
                    .get_slot(slot.id)?
                    .versions
                    .into_iter()
                    .find(|version| !version.retired);
                self.index
                    .set_active_version(slot.id, newest.map(|version| version.id))?;
            }
            return Ok(());
        };
        for (slot_uuid, slot) in &state.slots {
            let Some(slot_id) = self.index.slot_id(*slot_uuid)? else {
                continue;
            };
            let version = slot
                .active_version_uuid
                .filter(|version| !retired.contains(version));
            if let Some(version) = version
                && let Some(version_id) = self.index.version_id(version)?
            {
                self.index.set_active_version(slot_id, Some(version_id))?;
            }
            if state.active_slot_uuid == Some(*slot_uuid) {
                self.index.set_active_slot(game_id, slot_id)?;
            }
        }
        Ok(())
    }

    /// Brings back a deleted slot into its game, as an inactive slot pointing
    /// at its newest kept version, so switching to it later restores that
    /// rather than an empty folder. A name taken meanwhile gets a number.
    pub fn recover_slot(&self, uuid: Uuid, now: DateTime<Utc>) -> Result<()> {
        let (game, mut meta) = self
            .find_slot(uuid)?
            .context("This playthrough is no longer in your library")?;
        if !meta.deletion.is_recoverable(now) {
            bail!("'{}' can no longer be restored", meta.name);
        }
        let game_id = self
            .index
            .game_id(game.uuid)?
            .filter(|_| !game.deletion.is_deleted())
            .with_context(|| format!("'{}' isn't in your library", game.title))?;
        let full = self.index.get_game(game_id)?;
        let base = meta.name.clone();
        meta.name = (1..)
            .map(|n| match n {
                1 => base.clone(),
                n => format!("{base} ({n})"),
            })
            .find_map(|name| self.validate_slot_name(&full, &name, None).ok())
            .context("no free playthrough name")?;
        meta.deletion.recover(now);
        meta.updated_at = now;
        self.store.write_slot(&game.folder_slug, &meta)?;
        self.refresh_game(game.uuid)?;
        let slot_id = self
            .index
            .slot_id(uuid)?
            .context("Recovered playthrough is not indexed")?;
        let newest = self
            .index
            .get_slot(slot_id)?
            .versions
            .into_iter()
            .find(|version| !version.retired);
        self.index
            .set_active_version(slot_id, newest.map(|version| version.id))
    }

    /// Ends the recovery window of a deleted game or slot now. Its history is
    /// collected by the next maintenance; the metadata stays so every device
    /// learns of the purge.
    pub fn purge_deleted(&self, uuid: Uuid, now: DateTime<Utc>) -> Result<()> {
        if let Some(mut meta) = self.store.find_game(uuid)? {
            if !meta.deletion.is_deleted() {
                bail!("'{}' isn't deleted", meta.title);
            }
            meta.deletion.purge(now);
            meta.updated_at = now;
            return self.store.write_game(&meta);
        }
        let (game, mut meta) = self
            .find_slot(uuid)?
            .context("This item is no longer in your library")?;
        if !meta.deletion.is_deleted() {
            bail!("'{}' isn't deleted", meta.name);
        }
        meta.deletion.purge(now);
        meta.updated_at = now;
        self.store.write_slot(&game.folder_slug, &meta)
    }

    pub(crate) fn find_slot(&self, uuid: Uuid) -> Result<Option<(GameMeta, SlotMeta)>> {
        for game in self.store.game_metas()? {
            if let Some(slot) = self
                .store
                .slot_metas(&game)?
                .into_iter()
                .find(|slot| slot.uuid == uuid)
            {
                return Ok(Some((game, slot)));
            }
        }
        Ok(None)
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

    /// Tags version `id` with `tag` unless its metadata, the truth, already has one. Call it under
    /// the repository lock, so no other tag lands between the read and the write.
    pub fn tag_if_untagged(&self, id: i32, tag: &str) -> Result<Version> {
        let version = self.index.get_version(id)?;
        if self.version_meta(&version)?.tag.is_some() {
            return Ok(version.version);
        }
        self.tag_version(id, Some(tag.to_owned()))
    }

    /// The parent `version` branches off, when that parent already has
    /// another child in the slot. Read under the repository lock that
    /// created `version`, so no other version lands in between.
    pub fn branch_from(&self, version: &Version) -> Result<Option<crate::detection::BranchFrom>> {
        let Some(parent_id) = version.parent_id else {
            return Ok(None);
        };
        let slot = self.index.get_slot(version.slot_id)?;
        let forks = slot
            .versions
            .iter()
            .any(|other| other.parent_id == Some(parent_id) && other.id != version.id);
        Ok(slot
            .versions
            .iter()
            .find(|parent| forks && parent.id == parent_id)
            .map(|parent| crate::detection::BranchFrom {
                version_number: parent.version_number,
                tag: parent.tag.clone(),
            }))
    }

    /// Captures the live tracked directories and records a new version on the
    /// active slot unless the content equals the active version. The caller
    /// must hold the repository lock.
    /// `limited` refuses saves far larger than the game's usually are (see
    /// `capture_limit`), before any file is read.
    pub fn checkpoint_locked(
        &self,
        game: &FullGame,
        reason: VersionReason,
        limited: bool,
    ) -> Result<CheckpointOutcome> {
        let slot = game
            .active_slot
            .as_ref()
            .context("game has no active slot")?;
        let limit = limited.then(|| self.capture_limit(game));
        let files = self.capture(game, limit)?;
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
        let now = Utc::now();
        // The first capture may be delayed until saves exist. Starting another
        // slot carries known activity forward rather than dating restored files.
        let first_capture = game
            .slot_summaries
            .iter()
            .all(|slot| slot.version_count == 0);
        let last_played_at = if reason == VersionReason::Initial || first_capture {
            game.last_played_at.map(|at| at.and_utc()).or_else(|| {
                files
                    .iter()
                    .filter_map(|file| {
                        let dir = game
                            .tracked_dirs
                            .iter()
                            .find(|dir| dir.uuid == file.tracked_dir_uuid.to_string())?;
                        let modified = std::fs::metadata(Path::new(&dir.path).join(&file.path))
                            .ok()?
                            .modified()
                            .ok()?;
                        if modified < std::time::UNIX_EPOCH {
                            return None;
                        }
                        Some(DateTime::<Utc>::from(modified).min(now))
                    })
                    .max()
            })
        } else {
            Some(now)
        };
        self.commit_version(game, slot, parent_uuid, reason, files, now, last_played_at)
            .map(CheckpointOutcome::Created)
    }

    /// The live contents of the tracked directories, stored as chunks. The
    /// caller must hold the repository lock.
    pub fn capture(&self, game: &FullGame, limit: Option<chunks::Limit>) -> Result<Vec<FileEntry>> {
        with_roots(game, |roots| chunks::capture(&self.store, roots, limit))
            .map_err(crate::error::tag_in_use)
            .context("failed to capture tracked directories")
    }

    /// How much the game's saves may hold before a capture refuses them: ten
    /// times its active version, and never less than `MIN_CAPTURE_LIMIT`. Saves
    /// rarely grow that fast; a mistaken folder (all of AppData) is caught
    /// before a single file is read.
    pub fn capture_limit(&self, game: &FullGame) -> chunks::Limit {
        let active = match (&game.active_slot, &game.last_version) {
            (Some(slot), Some(version)) => self
                .store
                .version_meta(
                    &game.game.folder_slug,
                    &slot.folder_slug,
                    &version.folder_slug,
                )
                .ok(),
            _ => None,
        };
        let (bytes, files) = active.map_or((0, 0), |meta| {
            (
                meta.files.iter().map(|file| file.size).sum(),
                meta.files.len(),
            )
        });
        chunks::Limit {
            bytes: MIN_CAPTURE_LIMIT.bytes.max(bytes.saturating_mul(10)),
            files: MIN_CAPTURE_LIMIT.files.max(files.saturating_mul(10)),
        }
    }

    /// Whether the live saves differ from the active version, by content, as
    /// a checkpoint would find them; nothing is written. A file being written
    /// while it is read differs.
    pub fn live_differs(&self, game: &FullGame) -> Result<bool> {
        let limit = Some(self.capture_limit(game));
        let files = match with_roots(game, |roots| chunks::hash(roots, limit)) {
            Err(error) if error.is::<chunks::KeptChanging>() => return Ok(true),
            // Far larger than the active version: they differ, and aren't read.
            Err(error)
                if crate::error::error_kind(&error) == Some(crate::error::ErrorKind::TooLarge) =>
            {
                return Ok(true);
            }
            files => files?,
        };
        let (Some(slot), Some(active)) = (&game.active_slot, &game.last_version) else {
            return Ok(!files.is_empty());
        };
        let meta = self.store.version_meta(
            &game.game.folder_slug,
            &slot.folder_slug,
            &active.folder_slug,
        )?;
        // Both sorted alike; the hashed files have no chunk list to compare.
        let same = |live: &FileEntry, recorded: &FileEntry| {
            (live.tracked_dir_uuid, &live.path, live.size, &live.sha256)
                == (
                    recorded.tracked_dir_uuid,
                    &recorded.path,
                    recorded.size,
                    &recorded.sha256,
                )
        };
        Ok(files.len() != meta.files.len()
            || !files.iter().zip(&meta.files).all(|(a, b)| same(a, b)))
    }

    /// Records `files`, captured at `created_at`, as a new version on `slot`
    /// and makes it the slot's active one. The caller must hold the
    /// repository lock.
    #[allow(clippy::too_many_arguments)]
    pub fn commit_version(
        &self,
        game: &FullGame,
        slot: &Slot,
        parent_uuid: Option<Uuid>,
        reason: VersionReason,
        files: Vec<FileEntry>,
        created_at: DateTime<Utc>,
        last_played_at: Option<DateTime<Utc>>,
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
            last_played_at,
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
                deletion: Default::default(),
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
            return Err(anyhow::Error::new(ErrorKind::NameTaken)
                .context(format!("A game named '{title}' already exists")));
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

    /// Tracked directories must be absolute, not shared with other apps, and
    /// not overlap another game's.
    fn validate_dirs(&self, id: Option<i32>, dirs: &[String]) -> Result<()> {
        let taken = self.index.tracked_paths()?;
        for dir in dirs {
            if !Path::new(dir).is_absolute() {
                bail!("Tracked directory must be an absolute path: {dir}");
            }
            if crate::path_placeholders::is_shared_root(Path::new(dir)) {
                return Err(anyhow::Error::new(ErrorKind::SharedFolder).context(format!(
                    "{dir} holds other apps' files too; track the game's own save folder inside it"
                )));
            }
            if let Some((_, path)) = taken
                .iter()
                .find(|(game_id, path)| Some(*game_id) != id && overlapping_paths(path, dir))
            {
                return Err(anyhow::Error::new(ErrorKind::FolderTaken)
                    .context(format!("{dir} overlaps {path}, which another game tracks")));
            }
        }
        Ok(())
    }
}

/// The least a capture reads before refusing: game saves are almost always
/// far smaller (95% of 62 Steam Cloud folders sampled were under 36 MB, the
/// largest 395 MB in 27 files).
pub const MIN_CAPTURE_LIMIT: chunks::Limit = chunks::Limit {
    bytes: 1_000_000_000,
    files: 10_000,
};

/// Runs `walk` over the game's tracked directories with their exclusions.
fn with_roots<T>(game: &FullGame, walk: impl FnOnce(&[CaptureRoot]) -> Result<T>) -> Result<T> {
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
    walk(&roots)
}

/// When the recovery window of a still recoverable item ends.
fn recoverable_until(deletion: &Deletion, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    deletion.expires_at().filter(|end| now < *end)
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
