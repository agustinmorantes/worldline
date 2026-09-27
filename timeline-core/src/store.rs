//! The on-disk history format shared by the local repository, the sync remote
//! and export bundles. Repository files are the source of truth (ADR 0001);
//! this module owns their layout, their types and every validator.

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

pub const FORMAT_VERSION: u32 = 3;
const METADATA: &str = "metadata.json";
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
    #[serde(default)]
    pub deleted_at: Option<DateTime<Utc>>,
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
    #[serde(default)]
    pub deleted_at: Option<DateTime<Utc>>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
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
            ..meta.clone()
        };
        strip(self) == strip(other)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetirementMarker {
    pub version_uuid: Uuid,
    pub slot_uuid: Uuid,
    pub game_uuid: Uuid,
    pub retired_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SlotRecord {
    pub meta: SlotMeta,
    pub versions: Vec<VersionMeta>,
}

#[derive(Debug, Clone, PartialEq)]
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
                app: "savegame-timeline".into(),
                created_at: Utc::now(),
            },
        )?;
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    /// Opens a store that must already carry a current format marker.
    pub fn open_existing(root: &Path) -> Result<Self> {
        let meta: StoreMeta = read_json(&root.join(METADATA))
            .with_context(|| format!("{} is not a history store", root.display()))?;
        if meta.format_version != FORMAT_VERSION {
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

    pub fn root(&self) -> &Path {
        &self.root
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

    /// Every stored chunk with its on-disk (compressed) size. Anything that is
    /// not a well-formed chunk path is ignored.
    pub fn chunk_sizes(&self) -> Result<HashMap<String, u64>> {
        let mut sizes = HashMap::new();
        for shard in entries(&self.root.join("chunks"))? {
            let Some(prefix) = well_formed_name(&shard, 2) else {
                continue;
            };
            for chunk in entries(&shard)? {
                let Some(rest) = well_formed_name(&chunk, 62) else {
                    continue;
                };
                let metadata = fs::symlink_metadata(&chunk)
                    .with_context(|| format!("failed to read {}", chunk.display()))?;
                if metadata.is_file() {
                    sizes.insert(format!("sha256:{prefix}{rest}"), metadata.len());
                }
            }
        }
        Ok(sizes)
    }

    /// The first `metadata.json` that exists but cannot be read. Destructive
    /// maintenance refuses to run while one exists (fail closed).
    pub fn first_unreadable_metadata(&self) -> Result<Option<PathBuf>> {
        fn walk(dir: &Path, depth: usize) -> Result<Option<PathBuf>> {
            for child in entity_dirs(dir)? {
                let metadata = child.join(METADATA);
                if metadata.exists() && read_json::<serde_json::Value>(&metadata).is_err() {
                    return Ok(Some(metadata));
                }
                if depth > 0 {
                    for sub in ["tracked-dirs", "slots", "versions"] {
                        if let Some(found) = walk(&child.join(sub), depth - 1)? {
                            return Ok(Some(found));
                        }
                    }
                }
            }
            Ok(None)
        }
        walk(&self.root.join("games"), 2)
    }

    /// Game metadata only, without slots or versions.
    pub fn game_metas(&self) -> Result<Vec<GameMeta>> {
        Ok(entity_dirs(&self.root.join("games"))?
            .into_iter()
            .filter_map(|dir| read_entity::<GameMeta>(&dir))
            .filter(|meta| valid_slug(&meta.folder_slug))
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

    pub fn game(&self, uuid: Uuid) -> Result<Option<GameRecord>> {
        self.find_game(uuid)?
            .map(|meta| self.load_game(meta))
            .transpose()
    }

    /// The game stored under `slug`, if its metadata is readable.
    pub fn game_by_slug(&self, slug: &str) -> Result<Option<GameRecord>> {
        ensure_slug(slug)?;
        read_entity::<GameMeta>(&self.game_dir(slug))
            .filter(|meta| meta.folder_slug == slug)
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
            .filter(|tracked| tracked.game_uuid == meta.uuid && valid_slug(&tracked.folder_slug))
            .collect();
        let mut slots = Vec::new();
        for slot_dir in entity_dirs(&dir.join("slots"))? {
            let Some(slot) = read_entity::<SlotMeta>(&slot_dir) else {
                continue;
            };
            if slot.game_uuid != meta.uuid || !valid_slug(&slot.folder_slug) {
                continue;
            }
            let mut versions: Vec<VersionMeta> = entity_dirs(&slot_dir.join("versions"))?
                .into_iter()
                .filter_map(|dir| read_entity::<VersionMeta>(&dir))
                .filter(|version| {
                    version.slot_uuid == slot.uuid
                        && version.game_uuid == meta.uuid
                        && valid_slug(&version.folder_slug)
                        && validate_manifest(version).is_ok()
                })
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

    /// Writes game metadata, merging last-writer-wins with a sticky tombstone.
    pub fn write_game(&self, meta: &GameMeta) -> Result<()> {
        ensure_slug(&meta.folder_slug)?;
        let dir = self.game_dir(&meta.folder_slug);
        merge_write(&dir, meta, |existing: &GameMeta, incoming| {
            ensure_same(existing.uuid, incoming.uuid)?;
            let mut merged = newer(existing, incoming, |m| m.updated_at).clone();
            merged.deleted_at = existing.deleted_at.or(incoming.deleted_at);
            merged.folder_slug = existing.folder_slug.clone();
            merged.created_at = existing.created_at;
            Ok(merged)
        })
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
            merged.deleted_at = existing.deleted_at.or(incoming.deleted_at);
            merged.folder_slug = existing.folder_slug.clone();
            Ok(merged)
        })
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
            if meta.updated_at > existing.updated_at && meta.tag != existing.tag {
                let mut merged = existing;
                merged.tag = meta.tag.clone();
                merged.updated_at = meta.updated_at;
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

fn read_entity<T: DeserializeOwned>(dir: &Path) -> Option<T> {
    read_json(&dir.join(METADATA)).ok()
}

/// Child directories that could be entities: stray files, dot-entries,
/// staging dirs and cloud-client litter are skipped.
fn entity_dirs(parent: &Path) -> Result<Vec<PathBuf>> {
    Ok(entries(parent)?
        .into_iter()
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(valid_slug)
                && fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir())
        })
        .collect())
}

/// Directory entries, or none when the directory is missing.
pub(crate) fn entries(parent: &Path) -> Result<Vec<PathBuf>> {
    match fs::read_dir(parent) {
        Ok(read) => Ok(read
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .collect()),
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
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    let mut payload = serde_json::to_vec_pretty(value)?;
    payload.push(b'\n');
    let tmp = parent.join(format!(".tmp-{}", short_random()));
    let result = (|| -> Result<()> {
        let mut file =
            File::create(&tmp).with_context(|| format!("failed to create {}", tmp.display()))?;
        file.write_all(&payload)?;
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
fn atomic_replace(source: &Path, destination: &Path) -> io::Result<()> {
    move_file(source, destination, true)
}

#[cfg(windows)]
pub fn rename_no_replace(source: &Path, destination: &Path) -> io::Result<()> {
    move_file(source, destination, false)
}

#[cfg(target_os = "linux")]
fn atomic_replace(source: &Path, destination: &Path) -> io::Result<()> {
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
            deleted_at: None,
        }
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
        deleted.deleted_at = Some(t0);
        store.write_game(&deleted).unwrap();
        // A later rename from a device that never saw the deletion.
        store
            .write_game(&game(uuid, "Renamed", t0 + chrono::Duration::seconds(5)))
            .unwrap();
        let stored = store.find_game(uuid).unwrap().unwrap();
        assert_eq!(stored.title, "Renamed");
        assert!(stored.deleted_at.is_some());
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
        tagged.updated_at = now + chrono::Duration::seconds(1);
        store.publish_version("g__g_1", "s__s_1", &tagged).unwrap();
        // A stale copy without the tag doesn't erase it.
        store.publish_version("g__g_1", "s__s_1", &version).unwrap();
        let mut different = version.clone();
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
                app: "savegame-timeline".into(),
                created_at: Utc::now(),
            },
        )
        .unwrap();
        assert!(Store::open(root.path()).is_err());
    }
}
