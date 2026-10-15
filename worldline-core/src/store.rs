//! The on-disk history format shared by the local repository, the sync remote
//! and export bundles. Repository files are the source of truth (ADR 0001);
//! this module owns their layout, their types and every validator.

use crate::error::ErrorKind;
use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

/// 4 added recoverable deletes (ADR 0011); a format 3 store is upgraded in place.
pub const FORMAT_VERSION: u32 = 4;
/// How long a deleted game or slot can be recovered (ADR 0011).
pub const RECOVERY_WINDOW: chrono::Duration = chrono::Duration::days(30);
const METADATA: &str = "metadata.json";
/// The game's icon, normalized to 256 px PNG (ADR 0005).
const ICON: &str = "icon.png";
/// A version's screenshot, in the version's folder. It usually arrives after
/// the version is published, so it isn't part of the (immutable) metadata.
const SCREENSHOT: &str = "screenshot.jpg";
const RESERVED_WINDOWS_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoreMeta {
    pub format_version: u32,
    pub app: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GameMeta {
    pub uuid: Uuid,
    pub title: String,
    pub folder_slug: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(flatten)]
    pub deletion: Deletion,
    /// Hash of the game folder's `icon.png`; `None` when the game has no icon.
    #[serde(default)]
    pub icon_sha256: Option<String>,
    /// Accent hue (0–359); `None` falls back to one derived from the UUID.
    #[serde(default)]
    pub accent_hue: Option<u16>,
    #[serde(default)]
    pub accent_hue_source: AccentHueSource,
    #[serde(default)]
    pub icon_source: IconSource,
}

/// Who chose the icon: found automatically (a sharper automatic one may
/// replace it), or picked by the user (never replaced automatically).
/// Metadata written before this existed counts as the user's: nothing tells
/// whether its icon was picked by hand.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum IconSource {
    Auto,
    #[default]
    User,
}

/// Whether a game or slot is deleted, and whether it can still be recovered
/// (ADR 0011). Each mark merges on its own, so deleting again after a
/// recovery and recovering again after a delete both work whatever
/// `updated_at` says, and a purge is final.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Deletion {
    #[serde(default)]
    pub deleted_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovered_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purged_at: Option<DateTime<Utc>>,
}

impl Deletion {
    /// Purged, or deleted after its last recovery (`None` is earliest).
    pub fn is_deleted(&self) -> bool {
        self.purged_at.is_some() || self.deleted_at > self.recovered_at
    }

    /// When the recovery window of a deleted, unpurged item ends.
    pub fn expires_at(&self) -> Option<DateTime<Utc>> {
        self.deleted_at
            .filter(|_| self.is_deleted() && self.purged_at.is_none())
            .map(|at| at + RECOVERY_WINDOW)
    }

    pub fn is_recoverable(&self, now: DateTime<Utc>) -> bool {
        self.expires_at().is_some_and(|end| now < end)
    }

    /// Live, or deleted but still recoverable: its history is kept.
    pub fn keeps_history(&self, now: DateTime<Utc>) -> bool {
        !self.is_deleted() || self.is_recoverable(now)
    }

    /// Lands after the last recovery, even one from a device whose clock ran ahead.
    pub fn delete(&mut self, now: DateTime<Utc>) {
        self.deleted_at = Some(after(now, self.recovered_at));
    }

    /// Lands after the last delete, even one from a device whose clock ran ahead.
    pub fn recover(&mut self, now: DateTime<Utc>) {
        self.recovered_at = Some(after(now, self.deleted_at));
    }

    pub fn purge(&mut self, now: DateTime<Utc>) {
        self.purged_at = Some(now);
    }

    /// The later delete and the later recovery win; a purge stays (the earliest).
    pub fn merge(a: Self, b: Self) -> Self {
        Self {
            deleted_at: a.deleted_at.max(b.deleted_at),
            recovered_at: a.recovered_at.max(b.recovered_at),
            purged_at: match (a.purged_at, b.purged_at) {
                (Some(x), Some(y)) => Some(x.min(y)),
                (x, y) => x.or(y),
            },
        }
    }
}

fn after(now: DateTime<Utc>, other: Option<DateTime<Utc>>) -> DateTime<Utc> {
    other.map_or(now, |other| {
        now.max(other + chrono::Duration::milliseconds(1))
    })
}

/// Who chose the accent hue: computed from the icon, or picked by the user
/// (kept when the icon changes).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AccentHueSource {
    #[default]
    Auto,
    User,
}

impl AccentHueSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::User => "user",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrackedDirMeta {
    pub uuid: Uuid,
    pub game_uuid: Uuid,
    pub folder_slug: String,
    /// Absolute path with the longest matching placeholder root substituted.
    pub path: String,
    pub label: Option<String>,
    pub exclusions: Vec<String>,
    pub retired_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SlotMeta {
    pub uuid: Uuid,
    pub game_uuid: Uuid,
    pub folder_slug: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(flatten)]
    pub deletion: Deletion,
    /// Set by a deletion: the versions the deleting device had, so devices
    /// with progress it never saw keep that progress.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub seen_versions: Vec<Uuid>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VersionReason {
    Initial,
    Manual,
    PreSwitch,
    PreRestore,
    PreSlotCreate,
    Watcher,
}

impl VersionReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Initial => "initial",
            Self::Manual => "manual",
            Self::PreSwitch => "pre_switch",
            Self::PreRestore => "pre_restore",
            Self::PreSlotCreate => "pre_slot_create",
            Self::Watcher => "watcher",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileEntry {
    pub tracked_dir_uuid: Uuid,
    /// Relative path inside the tracked directory, `/`-separated.
    pub path: String,
    pub size: u64,
    /// SHA-256 of the whole file, for verification by hand.
    pub sha256: String,
    /// Chunk hashes whose decompressed bytes, concatenated, rebuild the file.
    pub chunks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VersionMeta {
    /// Estimated save activity, independent of when this version was recorded.
    #[serde(default)]
    pub last_played_at: Option<DateTime<Utc>>,
    pub uuid: Uuid,
    pub game_uuid: Uuid,
    pub slot_uuid: Uuid,
    pub parent_uuid: Option<Uuid>,
    /// Display hint only; imported branches can reuse numbers.
    pub version_number: i32,
    pub folder_slug: String,
    pub reason: VersionReason,
    pub tag: Option<String>,
    pub created_at: DateTime<Utc>,
    /// Bumped by tag edits, the only mutable field.
    pub updated_at: DateTime<Utc>,
    pub origin_device: Option<Uuid>,
    pub files: Vec<FileEntry>,
}

impl VersionMeta {
    pub fn chunk_hashes(&self) -> impl Iterator<Item = &str> {
        self.files
            .iter()
            .flat_map(|file| file.chunks.iter().map(String::as_str))
    }

    fn same_identity(&self, other: &Self) -> bool {
        let strip = |meta: &Self| Self {
            tag: None,
            updated_at: meta.created_at,
            origin_device: None,
            last_played_at: None,
            ..meta.clone()
        };
        strip(self) == strip(other)
            && self
                .last_played_at
                .zip(other.last_played_at)
                .is_none_or(|(a, b)| a == b)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetirementMarker {
    pub version_uuid: Uuid,
    pub slot_uuid: Uuid,
    pub game_uuid: Uuid,
    pub retired_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlotRecord {
    pub meta: SlotMeta,
    pub versions: Vec<VersionMeta>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameRecord {
    pub meta: GameMeta,
    pub tracked_dirs: Vec<TrackedDirMeta>,
    pub slots: Vec<SlotRecord>,
}

impl GameRecord {
    pub fn versions(&self) -> impl Iterator<Item = &VersionMeta> {
        self.slots.iter().flat_map(|slot| &slot.versions)
    }

    pub fn version(&self, uuid: Uuid) -> Option<&VersionMeta> {
        self.versions().find(|version| version.uuid == uuid)
    }
}

/// A directory in the shared history format.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// Opens a store, creating the format marker when the directory is new.
    pub fn open(root: &Path) -> Result<Self> {
        fs::create_dir_all(root)
            .with_context(|| format!("failed to create store at {}", root.display()))?;
        let marker = root.join(METADATA);
        if marker.exists() {
            return Self::open_existing(root);
        }
        write_json(
            &marker,
            &StoreMeta {
                format_version: FORMAT_VERSION,
                app: "worldline".into(),
                created_at: Utc::now(),
            },
        )?;
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    /// Opens a store that must already carry a format marker. Format 3 is
    /// upgraded in place before anything else is read or written (its
    /// metadata is format 4 without the recovery marks); a newer format
    /// means this app is too old for it.
    pub fn open_existing(root: &Path) -> Result<Self> {
        let path = root.join(METADATA);
        let meta: StoreMeta = read_json(&path)
            .with_context(|| format!("{} is not a history store", root.display()))?;
        if meta.format_version == 3 {
            write_json(
                &path,
                &StoreMeta {
                    format_version: FORMAT_VERSION,
                    ..meta
                },
            )?;
        } else if meta.format_version > FORMAT_VERSION {
            return Err(
                anyhow::Error::new(ErrorKind::UpdateRequired).context(format!(
                    "store format {} at {} is newer than this app supports ({FORMAT_VERSION})",
                    meta.format_version,
                    root.display()
                )),
            );
        } else if meta.format_version != FORMAT_VERSION {
            bail!(
                "store format {} at {} is unsupported (expected {FORMAT_VERSION})",
                meta.format_version,
                root.display()
            );
        }
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    /// Fails unless the marker still names this app's format, so nothing is
    /// collected under rules a newer app has replaced since this store opened.
    pub fn check_format(&self) -> Result<()> {
        Self::open_existing(&self.root).map(drop)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// When the store was created: a store recreated at the same path is
    /// another store.
    pub fn created_at(&self) -> Result<DateTime<Utc>> {
        Ok(read_json::<StoreMeta>(&self.root.join(METADATA))?.created_at)
    }

    pub fn game_dir(&self, game_slug: &str) -> PathBuf {
        self.root.join("games").join(game_slug)
    }

    fn slot_dir(&self, game_slug: &str, slot_slug: &str) -> PathBuf {
        self.game_dir(game_slug).join("slots").join(slot_slug)
    }

    pub fn chunk_path(&self, hash: &str) -> Result<PathBuf> {
        let digest = parse_hash(hash)?;
        Ok(self
            .root
            .join("chunks")
            .join(&digest[..2])
            .join(&digest[2..]))
    }

    pub fn has_chunk(&self, hash: &str) -> bool {
        self.chunk_path(hash)
            .and_then(|path| Ok(fs::symlink_metadata(path)?))
            .is_ok_and(|metadata| metadata.is_file())
    }

    /// Moves a damaged chunk out of the way (dot-prefixed, so `has_chunk` and
    /// `chunk_sizes` no longer see it) and lets the next capture or upload
    /// republish it.
    pub fn quarantine_chunk(&self, hash: &str) -> Result<()> {
        let path = self.chunk_path(hash)?;
        let aside = path.with_file_name(format!(".corrupt-{}", short_random()));
        fs::rename(&path, &aside).with_context(|| format!("failed to set aside chunk {hash}"))
    }

    /// Every stored chunk with its on-disk (compressed) size. Anything that is
    /// not a well-formed chunk path is ignored. Sizes come from the directory
    /// listing (free on Windows), not a stat per chunk: on a cloud-drive remote
    /// that is one round trip per shard instead of one per chunk.
    pub fn chunk_sizes(&self) -> Result<HashMap<String, u64>> {
        let mut sizes = HashMap::new();
        for shard in entries(&self.root.join("chunks"))? {
            let Some(prefix) = well_formed_name(&shard, 2) else {
                continue;
            };
            for chunk in dir_entries(&shard)? {
                let Some(rest) = well_formed_name(&chunk.path(), 62) else {
                    continue;
                };
                let metadata = chunk
                    .metadata()
                    .with_context(|| format!("failed to read {}", chunk.path().display()))?;
                if metadata.is_file() {
                    sizes.insert(format!("sha256:{prefix}{rest}"), metadata.len());
                }
            }
        }
        Ok(sizes)
    }

    /// The first `metadata.json` that exists but that the catalog loaders would
    /// drop (unparseable, invalid, or not linked to its parent). Destructive
    /// maintenance refuses to run while one exists (fail closed): what the
    /// catalog cannot see, GC would treat as dead. A missing `metadata.json`
    /// is a half-arrived directory, not an error.
    pub fn first_unloadable_metadata(&self) -> Result<Option<PathBuf>> {
        for game_dir in entity_dirs(&self.root.join("games"))? {
            let game = match entity_state::<GameMeta>(&game_dir, game_loadable) {
                Ok(Some(game)) => game,
                Ok(None) => continue,
                Err(path) => return Ok(Some(path)),
            };
            for dir in entity_dirs(&game_dir.join("tracked-dirs"))? {
                if let Err(path) = entity_state::<TrackedDirMeta>(&dir, |tracked| {
                    tracked_dir_loadable(tracked, &game)
                }) {
                    return Ok(Some(path));
                }
            }
            for slot_dir in entity_dirs(&game_dir.join("slots"))? {
                let slot =
                    match entity_state::<SlotMeta>(&slot_dir, |slot| slot_loadable(slot, &game)) {
                        Ok(Some(slot)) => slot,
                        Ok(None) => continue,
                        Err(path) => return Ok(Some(path)),
                    };
                for dir in entity_dirs(&slot_dir.join("versions"))? {
                    if let Err(path) = entity_state::<VersionMeta>(&dir, |version| {
                        version_loadable(version, &slot, &game)
                    }) {
                        return Ok(Some(path));
                    }
                }
            }
        }
        Ok(None)
    }

    /// Game metadata only, without slots or versions.
    pub fn game_metas(&self) -> Result<Vec<GameMeta>> {
        Ok(entity_dirs(&self.root.join("games"))?
            .into_iter()
            .filter_map(|dir| read_entity::<GameMeta>(&dir))
            .filter(game_loadable)
            .collect())
    }

    /// The folder names under `games/` that could hold a game, from one
    /// listing and without reading any metadata.
    pub fn game_slugs(&self) -> Result<Vec<String>> {
        Ok(entity_dirs(&self.root.join("games"))?
            .into_iter()
            .filter_map(|dir| Some(dir.file_name()?.to_str()?.to_string()))
            .collect())
    }

    pub fn find_game(&self, uuid: Uuid) -> Result<Option<GameMeta>> {
        Ok(self
            .game_metas()?
            .into_iter()
            .find(|meta| meta.uuid == uuid))
    }

    /// Every readable game with its slots and versions. Malformed or
    /// half-arrived entries are skipped; they are absent, not errors.
    pub fn catalog(&self) -> Result<Vec<GameRecord>> {
        self.game_metas()?
            .into_iter()
            .map(|meta| self.load_game(meta))
            .collect()
    }

    /// A game's slot metadata, without versions.
    pub fn slot_metas(&self, game: &GameMeta) -> Result<Vec<SlotMeta>> {
        Ok(
            entity_dirs(&self.game_dir(&game.folder_slug).join("slots"))?
                .into_iter()
                .filter_map(|dir| read_entity::<SlotMeta>(&dir))
                .filter(|slot| slot_loadable(slot, game))
                .collect(),
        )
    }

    pub fn game(&self, uuid: Uuid) -> Result<Option<GameRecord>> {
        self.find_game(uuid)?
            .map(|meta| self.load_game(meta))
            .transpose()
    }

    /// The game stored under `slug`, if its metadata is readable.
    pub fn game_by_slug(&self, slug: &str) -> Result<Option<GameRecord>> {
        ensure_slug(slug)?;
        read_entity::<GameMeta>(&self.game_dir(slug))
            .filter(|meta| meta.folder_slug == slug && game_loadable(meta))
            .map(|meta| self.load_game(meta))
            .transpose()
    }

    pub fn version_meta(
        &self,
        game_slug: &str,
        slot_slug: &str,
        version_slug: &str,
    ) -> Result<VersionMeta> {
        ensure_slug(game_slug)?;
        ensure_slug(slot_slug)?;
        ensure_slug(version_slug)?;
        let meta: VersionMeta = read_json(
            &self
                .slot_dir(game_slug, slot_slug)
                .join("versions")
                .join(version_slug)
                .join(METADATA),
        )?;
        validate_manifest(&meta)?;
        Ok(meta)
    }

    fn load_game(&self, meta: GameMeta) -> Result<GameRecord> {
        let dir = self.game_dir(&meta.folder_slug);
        let tracked_dirs = entity_dirs(&dir.join("tracked-dirs"))?
            .into_iter()
            .filter_map(|dir| read_entity::<TrackedDirMeta>(&dir))
            .filter(|tracked| tracked_dir_loadable(tracked, &meta))
            .collect();
        let mut slots = Vec::new();
        for slot_dir in entity_dirs(&dir.join("slots"))? {
            let Some(slot) = read_entity::<SlotMeta>(&slot_dir) else {
                continue;
            };
            if !slot_loadable(&slot, &meta) {
                continue;
            }
            let mut versions: Vec<VersionMeta> = entity_dirs(&slot_dir.join("versions"))?
                .into_iter()
                .filter_map(|dir| read_entity::<VersionMeta>(&dir))
                .filter(|version| version_loadable(version, &slot, &meta))
                .collect();
            versions.sort_by_key(|version| (version.created_at, version.uuid));
            slots.push(SlotRecord {
                meta: slot,
                versions,
            });
        }
        slots.sort_by_key(|slot| (slot.meta.created_at, slot.meta.uuid));
        Ok(GameRecord {
            meta,
            tracked_dirs,
            slots,
        })
    }

    /// Writes game metadata, merging last-writer-wins except for the deletion
    /// marks, which merge on their own.
    pub fn write_game(&self, meta: &GameMeta) -> Result<()> {
        ensure_slug(&meta.folder_slug)?;
        validate_appearance(meta)?;
        let dir = self.game_dir(&meta.folder_slug);
        merge_write(&dir, meta, |existing: &GameMeta, incoming| {
            ensure_same(existing.uuid, incoming.uuid)?;
            let mut merged = newer(existing, incoming, |m| m.updated_at).clone();
            merged.deletion = Deletion::merge(existing.deletion, incoming.deletion);
            merged.folder_slug = existing.folder_slug.clone();
            merged.created_at = existing.created_at;
            Ok(merged)
        })
    }

    /// The game metadata stored under `slug`, if readable.
    pub fn game_meta(&self, slug: &str) -> Result<Option<GameMeta>> {
        ensure_slug(slug)?;
        Ok(read_entity::<GameMeta>(&self.game_dir(slug)).filter(game_loadable))
    }

    pub fn icon_path(&self, game_slug: &str) -> PathBuf {
        self.game_dir(game_slug).join(ICON)
    }

    /// The stored icon's bytes, or `None` when it hasn't arrived.
    pub fn read_icon(&self, game_slug: &str) -> Result<Option<Vec<u8>>> {
        ensure_slug(game_slug)?;
        let path = self.icon_path(game_slug);
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error).with_context(|| format!("failed to read {}", path.display())),
        }
    }

    /// Atomically replaces the game's icon. It is written before the metadata
    /// naming its hash, so readers of this store see the icon first.
    pub fn write_icon(&self, game_slug: &str, png: &[u8]) -> Result<()> {
        ensure_slug(game_slug)?;
        write_bytes(&self.icon_path(game_slug), png)
    }

    pub fn screenshot_path(&self, game_slug: &str, slot_slug: &str, version_slug: &str) -> PathBuf {
        self.slot_dir(game_slug, slot_slug)
            .join("versions")
            .join(version_slug)
            .join(SCREENSHOT)
    }

    /// The versions of `game` that have a screenshot here.
    pub fn screenshot_versions(&self, game: &GameRecord) -> BTreeSet<Uuid> {
        game.slots
            .iter()
            .flat_map(|slot| {
                slot.versions.iter().filter(|version| {
                    self.screenshot_path(
                        &game.meta.folder_slug,
                        &slot.meta.folder_slug,
                        &version.folder_slug,
                    )
                    .exists()
                })
            })
            .map(|version| version.uuid)
            .collect()
    }

    /// Atomically stores the screenshot of a published version.
    pub fn write_screenshot(
        &self,
        game_slug: &str,
        slot_slug: &str,
        version_slug: &str,
        jpeg: &[u8],
    ) -> Result<()> {
        ensure_slug(game_slug)?;
        ensure_slug(slot_slug)?;
        ensure_slug(version_slug)?;
        let path = self.screenshot_path(game_slug, slot_slug, version_slug);
        // Writing into a missing folder would leave a half-arrived version.
        if !path.parent().is_some_and(Path::is_dir) {
            bail!("version {version_slug} is not published");
        }
        write_bytes(&path, jpeg)
    }

    pub fn write_tracked_dir(&self, game_slug: &str, meta: &TrackedDirMeta) -> Result<()> {
        ensure_slug(game_slug)?;
        ensure_slug(&meta.folder_slug)?;
        let dir = self
            .game_dir(game_slug)
            .join("tracked-dirs")
            .join(&meta.folder_slug);
        merge_write(&dir, meta, |existing: &TrackedDirMeta, incoming| {
            ensure_same(existing.uuid, incoming.uuid)?;
            Ok(newer(existing, incoming, |m| m.updated_at).clone())
        })
    }

    pub fn write_slot(&self, game_slug: &str, meta: &SlotMeta) -> Result<()> {
        ensure_slug(game_slug)?;
        ensure_slug(&meta.folder_slug)?;
        let dir = self.slot_dir(game_slug, &meta.folder_slug);
        merge_write(&dir, meta, |existing: &SlotMeta, incoming| {
            ensure_same(existing.uuid, incoming.uuid)?;
            let mut merged = newer(existing, incoming, |m| m.updated_at).clone();
            merged.deletion = Deletion::merge(existing.deletion, incoming.deletion);
            merged.folder_slug = existing.folder_slug.clone();
            merged.seen_versions = existing
                .seen_versions
                .iter()
                .chain(&incoming.seen_versions)
                .copied()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            Ok(merged)
        })
    }

    /// Fails when writing `game` here would be rejected part-way: a metadata
    /// path that belongs to another entity, or a version UUID already stored
    /// with different content. Nothing is written.
    pub fn check_conflicts(&self, game: &GameRecord) -> Result<()> {
        let slug = &game.meta.folder_slug;
        ensure_slug(slug)?;
        let dir = self.game_dir(slug);
        if let Some(existing) = read_entity::<GameMeta>(&dir) {
            ensure_same(existing.uuid, game.meta.uuid)?;
        }
        for tracked in &game.tracked_dirs {
            ensure_slug(&tracked.folder_slug)?;
            let path = dir.join("tracked-dirs").join(&tracked.folder_slug);
            if let Some(existing) = read_entity::<TrackedDirMeta>(&path) {
                ensure_same(existing.uuid, tracked.uuid)?;
            }
        }
        for slot in &game.slots {
            ensure_slug(&slot.meta.folder_slug)?;
            let slot_dir = self.slot_dir(slug, &slot.meta.folder_slug);
            if let Some(existing) = read_entity::<SlotMeta>(&slot_dir) {
                ensure_same(existing.uuid, slot.meta.uuid)?;
            }
            for version in &slot.versions {
                ensure_slug(&version.folder_slug)?;
                let path = slot_dir.join("versions").join(&version.folder_slug);
                if read_entity::<VersionMeta>(&path).is_some_and(|v| !v.same_identity(version)) {
                    bail!("version {} was reused with different content", version.uuid);
                }
            }
        }
        // A UUID names one entity: stored under another path, it is a different one.
        let incoming = self.entity_paths(game);
        for existing in self.catalog()? {
            for (uuid, path) in self.entity_paths(&existing) {
                if incoming.get(&uuid).is_some_and(|wanted| *wanted != path) {
                    bail!("{uuid} is already stored at {}", path.display());
                }
            }
        }
        Ok(())
    }

    /// Where each entity of `game` has its metadata, by UUID.
    fn entity_paths(&self, game: &GameRecord) -> HashMap<Uuid, PathBuf> {
        let dir = self.game_dir(&game.meta.folder_slug);
        let mut paths = HashMap::from([(game.meta.uuid, dir.clone())]);
        for tracked in &game.tracked_dirs {
            paths.insert(
                tracked.uuid,
                dir.join("tracked-dirs").join(&tracked.folder_slug),
            );
        }
        for slot in &game.slots {
            let slot_dir = self.slot_dir(&game.meta.folder_slug, &slot.meta.folder_slug);
            for version in &slot.versions {
                paths.insert(
                    version.uuid,
                    slot_dir.join("versions").join(&version.folder_slug),
                );
            }
            paths.insert(slot.meta.uuid, slot_dir);
        }
        paths
    }

    /// Publishes version metadata. The first write wins; later writes may only
    /// change the tag, last-writer-wins on `updated_at`.
    pub fn publish_version(
        &self,
        game_slug: &str,
        slot_slug: &str,
        meta: &VersionMeta,
    ) -> Result<()> {
        ensure_slug(game_slug)?;
        ensure_slug(slot_slug)?;
        ensure_slug(&meta.folder_slug)?;
        validate_manifest(meta)?;
        let dir = self
            .slot_dir(game_slug, slot_slug)
            .join("versions")
            .join(&meta.folder_slug);
        let path = dir.join(METADATA);
        if let Some(existing) = read_entity::<VersionMeta>(&dir) {
            if !existing.same_identity(meta) {
                bail!("version {} was reused with different content", meta.uuid);
            }
            let mut merged = existing.clone();
            merged.last_played_at = existing.last_played_at.or(meta.last_played_at);
            if meta.updated_at > existing.updated_at && meta.tag != existing.tag {
                merged.tag = meta.tag.clone();
                merged.updated_at = meta.updated_at;
            }
            if merged != existing {
                write_json(&path, &merged)?;
            }
            return Ok(());
        }
        // A directory without readable metadata is a half-arrived or crashed
        // publish: completing it in place is safe because the write is atomic.
        if dir.is_dir() {
            return write_json(&path, meta);
        }
        let parent = dir.parent().context("version dir has no parent")?;
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
        let staging = parent.join(format!(".tmp-{}", short_random()));
        fs::create_dir(&staging)?;
        let result = write_json(&staging.join(METADATA), meta)
            .and_then(|()| rename_no_replace(&staging, &dir).map_err(Into::into));
        if let Err(error) = result {
            let _ = fs::remove_dir_all(&staging);
            if read_entity::<VersionMeta>(&dir).is_some_and(|existing| existing.same_identity(meta))
            {
                return Ok(());
            }
            return Err(error).context("failed to publish version metadata");
        }
        sync_dir(parent);
        Ok(())
    }

    pub fn markers(&self) -> Result<HashMap<Uuid, RetirementMarker>> {
        let mut markers = HashMap::new();
        for path in entries(&self.root.join("retired-versions"))? {
            let Some(uuid) = json_file_uuid(&path) else {
                continue;
            };
            if let Ok(marker) = read_json::<RetirementMarker>(&path)
                && marker.version_uuid == uuid
            {
                markers.insert(uuid, marker);
            }
        }
        Ok(markers)
    }

    /// The versions with a marker file, from one listing and without reading
    /// the markers.
    pub fn marker_uuids(&self) -> Result<HashSet<Uuid>> {
        Ok(dir_entries(&self.root.join("retired-versions"))?
            .into_iter()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            .filter_map(|entry| {
                let name = entry.file_name();
                Uuid::parse_str(name.to_str()?.strip_suffix(".json")?).ok()
            })
            .collect())
    }

    /// One version's marker, if it is readable.
    pub fn marker(&self, version: Uuid) -> Option<RetirementMarker> {
        read_json::<RetirementMarker>(
            &self
                .root
                .join("retired-versions")
                .join(format!("{version}.json")),
        )
        .ok()
        .filter(|marker| marker.version_uuid == version)
    }

    /// Write-once: an existing marker for the same version is kept.
    pub fn write_marker(&self, marker: &RetirementMarker) -> Result<bool> {
        let path = self
            .root
            .join("retired-versions")
            .join(format!("{}.json", marker.version_uuid));
        if read_json::<RetirementMarker>(&path).is_ok() {
            return Ok(false);
        }
        write_json(&path, marker)?;
        Ok(true)
    }

    /// Unlike `write_marker`, overwrites: used when a game's identity changes
    /// and its markers must follow the versions to the new game.
    pub fn replace_marker(&self, marker: &RetirementMarker) -> Result<()> {
        write_json(
            &self
                .root
                .join("retired-versions")
                .join(format!("{}.json", marker.version_uuid)),
            marker,
        )
    }

    /// Removes a game's slots (with their versions) and tracked directories,
    /// keeping its own metadata (the tombstone of a superseded identity).
    pub fn drop_game_history(&self, game_slug: &str) -> Result<()> {
        ensure_slug(game_slug)?;
        for sub in ["slots", "tracked-dirs"] {
            let dir = self.game_dir(game_slug).join(sub);
            match fs::remove_dir_all(&dir) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => {
                    return Err(error)
                        .with_context(|| format!("failed to remove {}", dir.display()));
                }
                _ => {}
            }
        }
        Ok(())
    }
}

fn newer<'a, T>(existing: &'a T, incoming: &'a T, updated: impl Fn(&T) -> DateTime<Utc>) -> &'a T {
    if updated(incoming) > updated(existing) {
        incoming
    } else {
        existing
    }
}

fn ensure_same(existing: Uuid, incoming: Uuid) -> Result<()> {
    if existing != incoming {
        bail!("metadata path already belongs to {existing}, not {incoming}");
    }
    Ok(())
}

fn merge_write<T>(dir: &Path, incoming: &T, merge: impl Fn(&T, &T) -> Result<T>) -> Result<()>
where
    T: Serialize + DeserializeOwned + PartialEq,
{
    let value = match read_entity::<T>(dir) {
        Some(existing) => {
            let merged = merge(&existing, incoming)?;
            if merged == existing {
                return Ok(());
            }
            merged
        }
        None => return write_json(&dir.join(METADATA), incoming),
    };
    write_json(&dir.join(METADATA), &value)
}

/// The filters `load_game` applies; `first_unloadable_metadata` mirrors them.
fn game_loadable(game: &GameMeta) -> bool {
    valid_slug(&game.folder_slug) && validate_appearance(game).is_ok()
}

fn tracked_dir_loadable(tracked: &TrackedDirMeta, game: &GameMeta) -> bool {
    tracked.game_uuid == game.uuid && valid_slug(&tracked.folder_slug)
}

fn slot_loadable(slot: &SlotMeta, game: &GameMeta) -> bool {
    slot.game_uuid == game.uuid && valid_slug(&slot.folder_slug)
}

fn version_loadable(version: &VersionMeta, slot: &SlotMeta, game: &GameMeta) -> bool {
    version.slot_uuid == slot.uuid
        && version.game_uuid == game.uuid
        && valid_slug(&version.folder_slug)
        && validate_manifest(version).is_ok()
}

/// `Ok(None)`: no metadata yet. `Err(path)`: metadata the loaders would drop.
fn entity_state<T: DeserializeOwned>(
    dir: &Path,
    loadable: impl Fn(&T) -> bool,
) -> Result<Option<T>, PathBuf> {
    let path = dir.join(METADATA);
    if !path.exists() {
        return Ok(None);
    }
    match read_json::<T>(&path) {
        Ok(entity) if loadable(&entity) => Ok(Some(entity)),
        _ => Err(path),
    }
}

fn read_entity<T: DeserializeOwned>(dir: &Path) -> Option<T> {
    read_json(&dir.join(METADATA)).ok()
}

/// Child directories that could be entities: stray files, dot-entries,
/// staging dirs and cloud-client litter are skipped.
fn entity_dirs(parent: &Path) -> Result<Vec<PathBuf>> {
    Ok(dir_entries(parent)?
        .into_iter()
        .filter(|entry| {
            entry.file_name().to_str().is_some_and(valid_slug)
                // The listing's type, like `symlink_metadata`, doesn't follow links.
                && entry.file_type().is_ok_and(|kind| kind.is_dir())
        })
        .map(|entry| entry.path())
        .collect())
}

/// Directory entries, or none when the directory is missing.
pub(crate) fn entries(parent: &Path) -> Result<Vec<PathBuf>> {
    Ok(dir_entries(parent)?
        .into_iter()
        .map(|entry| entry.path())
        .collect())
}

/// `entries` with the type and metadata the listing already carries.
fn dir_entries(parent: &Path) -> Result<Vec<fs::DirEntry>> {
    match fs::read_dir(parent) {
        Ok(read) => Ok(read.filter_map(|entry| entry.ok()).collect()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error).with_context(|| format!("failed to list {}", parent.display())),
    }
}

/// `<uuid>.json` file names; anything else (conflict copies, temp files) is `None`.
pub(crate) fn json_file_uuid(path: &Path) -> Option<Uuid> {
    let name = path.file_name()?.to_str()?;
    let uuid = Uuid::parse_str(name.strip_suffix(".json")?).ok()?;
    fs::symlink_metadata(path)
        .ok()
        .filter(|metadata| metadata.is_file())
        .map(|_| uuid)
}

fn well_formed_name(path: &Path, len: usize) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    (name.len() == len
        && name
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
    .then(|| name.to_string())
}

// ---- validators (one per concept) -------------------------------------------

pub fn normalize_display_name(input: &str) -> String {
    input
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

pub fn slugify_name(input: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = false;
    for ch in input.trim().chars().flat_map(|c| c.to_lowercase()) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            last_was_dash = false;
        } else if !last_was_dash {
            slug.push('-');
            last_was_dash = true;
        }
    }
    let trimmed = slug.trim_matches('-');
    let mut normalized = if trimmed.is_empty() {
        "item".to_string()
    } else {
        trimmed
            .chars()
            .take(24)
            .collect::<String>()
            .trim_matches('-')
            .to_string()
    };
    if RESERVED_WINDOWS_NAMES.contains(&normalized.as_str()) {
        normalized = format!("x-{normalized}");
    }
    normalized
}

/// `<human>__<kind>_<first 8 hex of uuid>`; fixed at creation.
pub fn folder_slug(kind: char, human_name: &str, uuid: Uuid) -> String {
    format!(
        "{}__{kind}_{}",
        slugify_name(human_name),
        &uuid.simple().to_string()[..8]
    )
}

pub fn valid_slug(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('.')
        && value.len() <= 80
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

fn ensure_slug(value: &str) -> Result<()> {
    if !valid_slug(value) {
        bail!("invalid folder slug {value:?}");
    }
    Ok(())
}

pub fn parse_hash(hash: &str) -> Result<&str> {
    let digest = hash
        .strip_prefix("sha256:")
        .ok_or_else(|| anyhow!("unsupported hash {hash:?}"))?;
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        bail!("invalid SHA-256 hash {hash:?}");
    }
    Ok(digest)
}

pub fn format_hash(digest: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut hash = String::with_capacity(71);
    hash.push_str("sha256:");
    for byte in digest {
        let _ = write!(hash, "{byte:02x}");
    }
    hash
}

/// A manifest path must be relative, `/`-separated, and free of traversal.
pub fn validate_relative_path(path: &str) -> Result<()> {
    let parsed = Path::new(path);
    let safe = !path.is_empty()
        && !path.contains('\\')
        && !parsed.has_root()
        && parsed
            .components()
            .all(|c| matches!(c, Component::Normal(_)));
    if !safe {
        bail!("unsafe manifest path {path:?}");
    }
    Ok(())
}

/// The icon hash is a SHA-256 and the accent hue an angle below 360.
pub fn validate_appearance(game: &GameMeta) -> Result<()> {
    if let Some(hash) = &game.icon_sha256 {
        parse_hash(hash)?;
    }
    if let Some(hue) = game.accent_hue {
        validate_hue(hue)?;
    }
    Ok(())
}

pub fn validate_hue(hue: u16) -> Result<()> {
    if hue >= 360 {
        bail!("accent hue {hue} is out of range (0-359)");
    }
    Ok(())
}

pub fn validate_manifest(version: &VersionMeta) -> Result<()> {
    let mut previous: Option<(&Uuid, &str)> = None;
    for file in &version.files {
        validate_relative_path(&file.path)?;
        parse_hash(&file.sha256)?;
        for chunk in &file.chunks {
            parse_hash(chunk)?;
        }
        if file.chunks.is_empty() && file.size > 0 {
            bail!("manifest entry {} has no chunks", file.path);
        }
        let key = (&file.tracked_dir_uuid, file.path.as_str());
        if previous.is_some_and(|previous| previous >= key) {
            bail!(
                "manifest of version {} is not in canonical order",
                version.uuid
            );
        }
        previous = Some(key);
    }
    Ok(())
}

// ---- durable file operations -------------------------------------------------

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("failed to parse {}", path.display()))
}

/// Atomically replaces `path` with the JSON of `value` (temp file, fsync, rename).
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let mut payload = serde_json::to_vec_pretty(value)?;
    payload.push(b'\n');
    write_bytes(path, &payload)
}

/// Atomically replaces `path` with `payload` (temp file, fsync, rename).
fn write_bytes(path: &Path, payload: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    let tmp = parent.join(format!(".tmp-{}", short_random()));
    let result = (|| -> Result<()> {
        let mut file =
            File::create(&tmp).with_context(|| format!("failed to create {}", tmp.display()))?;
        file.write_all(payload)?;
        file.sync_all()?;
        drop(file);
        atomic_replace(&tmp, path)?;
        sync_dir(parent);
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result.with_context(|| format!("failed to write {}", path.display()))
}

/// Temp names stay short: Windows paths are limited to 260 characters.
pub fn short_random() -> String {
    Uuid::new_v4().simple().to_string()[..12].to_string()
}

/// Durable directory entry for a just-renamed file (Linux only; a no-op on Windows).
pub fn sync_dir(dir: &Path) {
    #[cfg(target_os = "linux")]
    if let Ok(handle) = File::open(dir) {
        let _ = handle.sync_all();
    }
    #[cfg(not(target_os = "linux"))]
    let _ = dir;
}

#[cfg(windows)]
fn move_file(source: &Path, destination: &Path, replace: bool) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let wide = |path: &Path| {
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>()
    };
    let (source, destination) = (wide(source), wide(destination));
    let flags = MOVEFILE_WRITE_THROUGH
        | if replace {
            MOVEFILE_REPLACE_EXISTING
        } else {
            0
        };
    for attempt in 0..8 {
        if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), flags) } != 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        // Antivirus and indexers briefly hold new files open.
        if error.kind() != io::ErrorKind::PermissionDenied || attempt == 7 {
            return Err(error);
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    unreachable!()
}

#[cfg(windows)]
pub fn atomic_replace(source: &Path, destination: &Path) -> io::Result<()> {
    move_file(source, destination, true)
}

#[cfg(windows)]
pub fn rename_no_replace(source: &Path, destination: &Path) -> io::Result<()> {
    move_file(source, destination, false)
}

#[cfg(target_os = "linux")]
pub fn atomic_replace(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(target_os = "linux")]
pub fn rename_no_replace(source: &Path, destination: &Path) -> io::Result<()> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};
    renameat_with(CWD, source, CWD, destination, RenameFlags::NOREPLACE).map_err(Into::into)
}

/// Serializes local captures, restores and garbage collection across processes.
pub fn with_lock<T>(root: &Path, action: impl FnOnce() -> Result<T>) -> Result<T> {
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join(".lock"))
        .context("failed to open repository lock")?;
    lock.lock().context("failed to lock repository")?;
    action()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn game(uuid: Uuid, title: &str, at: DateTime<Utc>) -> GameMeta {
        GameMeta {
            uuid,
            title: title.into(),
            folder_slug: folder_slug('g', "Game", uuid),
            created_at: at,
            updated_at: at,
            deletion: Default::default(),
            icon_sha256: None,
            accent_hue: None,
            accent_hue_source: AccentHueSource::Auto,
            icon_source: IconSource::User,
        }
    }

    #[test]
    fn an_icon_without_a_source_counts_as_the_users_and_auto_roundtrips() {
        let root = tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let uuid = Uuid::new_v4();
        let meta = GameMeta {
            icon_sha256: Some(crate::icon::sha256(b"png")),
            icon_source: IconSource::Auto,
            ..game(uuid, "Game", Utc::now())
        };
        let path = store.game_dir(&meta.folder_slug).join(METADATA);
        let mut old = serde_json::to_value(&meta).unwrap();
        old.as_object_mut().unwrap().remove("icon_source");
        write_json(&path, &old).unwrap();
        let loaded = store.find_game(uuid).unwrap().unwrap();
        assert_eq!(loaded.icon_source, IconSource::User);
        assert_eq!(loaded.icon_sha256, meta.icon_sha256);

        let auto = GameMeta {
            updated_at: meta.updated_at + chrono::Duration::seconds(1),
            ..meta
        };
        store.write_game(&auto).unwrap();
        assert!(
            fs::read_to_string(&path)
                .unwrap()
                .contains(r#""icon_source": "auto""#)
        );
        assert_eq!(store.find_game(uuid).unwrap().unwrap(), auto);
    }

    #[test]
    fn game_appearance_defaults_and_is_validated() {
        let root = tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let uuid = Uuid::new_v4();
        let meta = game(uuid, "Game", Utc::now());
        // Metadata written before icons existed still loads.
        let mut old = serde_json::to_value(&meta).unwrap();
        for field in [
            "icon_sha256",
            "accent_hue",
            "accent_hue_source",
            "icon_source",
        ] {
            old.as_object_mut().unwrap().remove(field);
        }
        write_json(&store.game_dir(&meta.folder_slug).join(METADATA), &old).unwrap();
        assert_eq!(store.find_game(uuid).unwrap().unwrap(), meta);

        let bad_hue = GameMeta {
            accent_hue: Some(360),
            ..meta.clone()
        };
        let bad_hash = GameMeta {
            icon_sha256: Some("sha256:xyz".into()),
            ..meta.clone()
        };
        assert!(store.write_game(&bad_hue).is_err());
        assert!(store.write_game(&bad_hash).is_err());
        write_json(&store.game_dir(&meta.folder_slug).join(METADATA), &bad_hue).unwrap();
        assert!(store.find_game(uuid).unwrap().is_none());
        assert!(store.first_unloadable_metadata().unwrap().is_some());

        let user = GameMeta {
            accent_hue: Some(359),
            accent_hue_source: AccentHueSource::User,
            updated_at: meta.updated_at + chrono::Duration::seconds(1),
            ..meta
        };
        store.write_game(&user).unwrap();
        let text = fs::read_to_string(store.game_dir(&user.folder_slug).join(METADATA)).unwrap();
        assert!(text.contains(r#""accent_hue_source": "user""#));
        store.write_icon(&user.folder_slug, b"png").unwrap();
        assert_eq!(
            store.read_icon(&user.folder_slug).unwrap().as_deref(),
            Some(&b"png"[..])
        );
    }

    #[test]
    fn slugs_are_readable_and_safe() {
        let uuid = Uuid::parse_str("8f3a2c9d-1111-2222-3333-444444444444").unwrap();
        assert_eq!(
            folder_slug('g', "Hollow Knight", uuid),
            "hollow-knight__g_8f3a2c9d"
        );
        assert_eq!(slugify_name("AUX"), "x-aux");
        assert_eq!(slugify_name("???"), "item");
        for bad in ["", ".", "..", "a/b", r"a\b", ".hidden", "Upper"] {
            assert!(!valid_slug(bad), "{bad}");
        }
    }

    #[test]
    fn game_metadata_merges_last_writer_wins_with_sticky_tombstone() {
        let root = tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let uuid = Uuid::new_v4();
        let t0 = Utc::now();
        store.write_game(&game(uuid, "Old", t0)).unwrap();
        let mut deleted = game(uuid, "Old", t0 + chrono::Duration::seconds(1));
        deleted.deletion.deleted_at = Some(t0);
        store.write_game(&deleted).unwrap();
        // A later rename from a device that never saw the deletion.
        store
            .write_game(&game(uuid, "Renamed", t0 + chrono::Duration::seconds(5)))
            .unwrap();
        let stored = store.find_game(uuid).unwrap().unwrap();
        assert_eq!(stored.title, "Renamed");
        assert!(stored.deletion.is_deleted());
        // An older write never wins.
        store.write_game(&game(uuid, "Stale", t0)).unwrap();
        assert_eq!(store.find_game(uuid).unwrap().unwrap().title, "Renamed");
    }

    #[test]
    fn catalog_skips_stray_and_malformed_entries() {
        let root = tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let uuid = Uuid::new_v4();
        let meta = game(uuid, "Game", Utc::now());
        store.write_game(&meta).unwrap();
        let games = root.path().join("games");
        fs::write(games.join("desktop.ini"), b"x").unwrap();
        fs::create_dir_all(games.join(".staging")).unwrap();
        fs::create_dir_all(games.join("broken__g_00000000")).unwrap();
        fs::write(games.join("broken__g_00000000").join(METADATA), b"{").unwrap();
        fs::create_dir_all(root.path().join("retired-versions")).unwrap();
        fs::write(
            root.path().join("retired-versions").join("x (1).json"),
            b"{",
        )
        .unwrap();
        let catalog = store.catalog().unwrap();
        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog[0].meta.uuid, uuid);
        assert!(store.markers().unwrap().is_empty());
    }

    #[test]
    fn version_metadata_is_write_once_except_for_newer_tags() {
        let root = tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let now = Utc::now();
        let version = VersionMeta {
            last_played_at: Some(now),
            uuid: Uuid::new_v4(),
            game_uuid: Uuid::new_v4(),
            slot_uuid: Uuid::new_v4(),
            parent_uuid: None,
            version_number: 1,
            folder_slug: "v001__v_00000001".into(),
            reason: VersionReason::Manual,
            tag: None,
            created_at: now,
            updated_at: now,
            origin_device: None,
            files: vec![],
        };
        store.publish_version("g__g_1", "s__s_1", &version).unwrap();
        let mut tagged = version.clone();
        tagged.tag = Some("boss".into());
        // A tag edit from a build without activity metadata keeps the date.
        tagged.last_played_at = None;
        tagged.updated_at = now + chrono::Duration::seconds(1);
        store.publish_version("g__g_1", "s__s_1", &tagged).unwrap();
        // A stale copy without the tag doesn't erase it.
        store.publish_version("g__g_1", "s__s_1", &version).unwrap();
        let mut different = version.clone();
        different.last_played_at = Some(now + chrono::Duration::seconds(1));
        assert!(
            store
                .publish_version("g__g_1", "s__s_1", &different)
                .is_err()
        );
        different = version.clone();
        different.version_number = 2;
        assert!(
            store
                .publish_version("g__g_1", "s__s_1", &different)
                .is_err()
        );
        let dir = store
            .slot_dir("g__g_1", "s__s_1")
            .join("versions")
            .join(&version.folder_slug);
        assert_eq!(
            read_entity::<VersionMeta>(&dir).unwrap().last_played_at,
            Some(now)
        );
        assert_eq!(
            read_entity::<VersionMeta>(&dir).unwrap().tag.as_deref(),
            Some("boss")
        );
    }

    #[test]
    fn manifest_validation_rejects_traversal_and_disorder() {
        for bad in ["", "../x", "/abs", "a/../b", r"a\b", "C:x"] {
            if cfg!(windows) || bad != "C:x" {
                assert!(validate_relative_path(bad).is_err(), "{bad}");
            }
        }
        assert!(validate_relative_path("dir/save.dat").is_ok());
    }

    #[test]
    fn rejects_other_formats() {
        let root = tempdir().unwrap();
        write_json(
            &root.path().join(METADATA),
            &StoreMeta {
                format_version: 2,
                app: "worldline".into(),
                created_at: Utc::now(),
            },
        )
        .unwrap();
        assert!(Store::open(root.path()).is_err());
    }

    #[test]
    fn format_3_is_upgraded_in_place_and_newer_formats_need_an_update() {
        let root = tempdir().unwrap();
        let marker = |format_version| StoreMeta {
            format_version,
            app: "worldline".into(),
            created_at: Utc::now(),
        };
        let path = root.path().join(METADATA);
        write_json(&path, &marker(3)).unwrap();
        // Format 3 metadata (a bare `deleted_at`) reads as a deletion.
        let uuid = Uuid::new_v4();
        let slug = folder_slug('g', "Old", uuid);
        fs::create_dir_all(root.path().join("games").join(&slug)).unwrap();
        fs::write(
            root.path().join("games").join(&slug).join(METADATA),
            format!(
                r#"{{"uuid":"{uuid}","title":"Old","folder_slug":"{slug}","created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z","deleted_at":"2026-01-02T00:00:00Z"}}"#
            ),
        )
        .unwrap();
        let store = Store::open_existing(root.path()).unwrap();
        let stored: StoreMeta = read_json(&path).unwrap();
        assert_eq!(stored.format_version, FORMAT_VERSION);
        assert!(
            store
                .find_game(uuid)
                .unwrap()
                .unwrap()
                .deletion
                .is_deleted()
        );

        write_json(&path, &marker(FORMAT_VERSION + 1)).unwrap();
        let error = Store::open_existing(root.path()).err().unwrap();
        assert_eq!(
            crate::error::error_kind(&error),
            Some(ErrorKind::UpdateRequired)
        );
        let untouched: StoreMeta = read_json(&path).unwrap();
        assert_eq!(untouched.format_version, FORMAT_VERSION + 1);
    }

    #[test]
    fn deletion_marks_merge_independently_and_a_purge_is_final() {
        let t0 = Utc::now();
        let at = |seconds| t0 + chrono::Duration::seconds(seconds);
        let mut deleted = Deletion::default();
        deleted.delete(at(0));
        let mut recovered = deleted;
        recovered.recover(at(1));
        assert!(!recovered.is_deleted());
        // A recovery beats the delete it saw, whichever side it merges from.
        assert!(!Deletion::merge(deleted, recovered).is_deleted());
        assert!(!Deletion::merge(recovered, deleted).is_deleted());
        // A later delete beats the recovery.
        let mut again = recovered;
        again.delete(at(2));
        assert!(Deletion::merge(recovered, again).is_deleted());
        // A recovery lands after a delete stamped by a clock running ahead.
        let mut ahead = Deletion::default();
        ahead.delete(at(60));
        ahead.recover(at(0));
        assert!(!ahead.is_deleted());
        // A purge wins over a concurrent recovery and ends the window.
        let mut purged = deleted;
        purged.purge(at(1));
        let merged = Deletion::merge(recovered, purged);
        assert!(merged.is_deleted());
        assert!(!merged.is_recoverable(at(2)));
        assert!(!merged.keeps_history(at(2)));
        // The window is 30 days from the delete.
        assert!(deleted.is_recoverable(at(0) + RECOVERY_WINDOW - chrono::Duration::seconds(1)));
        assert!(!deleted.is_recoverable(at(0) + RECOVERY_WINDOW));
        assert!(Deletion::default().keeps_history(at(0)));
    }

    #[test]
    fn markers_are_write_once_unless_explicitly_replaced_and_game_history_can_be_dropped() {
        let root = tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let (version, slot) = (Uuid::new_v4(), Uuid::new_v4());
        let marker = |game: Uuid| RetirementMarker {
            version_uuid: version,
            slot_uuid: slot,
            game_uuid: game,
            retired_at: Utc::now(),
        };
        let (old, new) = (Uuid::new_v4(), Uuid::new_v4());
        assert!(store.write_marker(&marker(old)).unwrap());
        assert!(!store.write_marker(&marker(new)).unwrap());
        assert_eq!(store.markers().unwrap()[&version].game_uuid, old);
        store.replace_marker(&marker(new)).unwrap();
        assert_eq!(store.markers().unwrap()[&version].game_uuid, new);

        let meta = game(old, "Old", Utc::now());
        store.write_game(&meta).unwrap();
        let dir = store.game_dir(&meta.folder_slug);
        fs::create_dir_all(dir.join("slots").join("s__s_1")).unwrap();
        fs::create_dir_all(dir.join("tracked-dirs").join("t__t_1")).unwrap();
        store.drop_game_history(&meta.folder_slug).unwrap();
        assert!(!dir.join("slots").exists() && !dir.join("tracked-dirs").exists());
        assert!(store.find_game(old).unwrap().is_some());
        store.drop_game_history(&meta.folder_slug).unwrap();
    }
}
