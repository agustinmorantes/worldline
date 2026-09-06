use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{self, BufReader, Write};
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

const RESERVED_WINDOWS_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];
const METADATA_FILE_NAME: &str = "metadata.json";
pub const REPOSITORY_FORMAT_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepositoryEntityKind {
    Game,
    Slot,
    Version,
    TrackedDir,
}

impl RepositoryEntityKind {
    fn short_code(self) -> &'static str {
        match self {
            Self::Game => "g",
            Self::Slot => "s",
            Self::Version => "v",
            Self::TrackedDir => "d",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepositoryMetadata {
    pub format_version: u32,
    pub created_at: DateTime<Utc>,
    pub app: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GameMetadata {
    pub entity: String,
    pub uuid: Uuid,
    pub title: String,
    pub folder_slug: String,
    pub active_slot_uuid: Option<Uuid>,
    pub icon: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrackedDirMetadata {
    pub entity: String,
    pub uuid: Uuid,
    pub game_uuid: Uuid,
    pub path: String,
    pub label: Option<String>,
    pub exclusions: Vec<String>,
    pub folder_slug: String,
    pub retired_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SlotMetadata {
    pub entity: String,
    pub uuid: Uuid,
    pub game_uuid: Uuid,
    pub name: String,
    pub folder_slug: String,
    pub active_version_uuid: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
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

    pub fn from_str_or_watcher(value: &str) -> Self {
        match value {
            "initial" => Self::Initial,
            "manual" => Self::Manual,
            "pre_switch" => Self::PreSwitch,
            "pre_restore" => Self::PreRestore,
            "pre_slot_create" => Self::PreSlotCreate,
            _ => Self::Watcher,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VersionFileMetadata {
    pub tracked_dir_uuid: Uuid,
    pub relative_path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VersionMetadata {
    pub entity: String,
    pub uuid: Uuid,
    pub game_uuid: Uuid,
    pub slot_uuid: Option<Uuid>,
    pub version_number: i32,
    pub folder_slug: String,
    pub parent_version_uuid: Option<Uuid>,
    pub tag: Option<String>,
    pub reason: VersionReason,
    pub created_at: DateTime<Utc>,
    pub content_hash: String,
    pub files: Vec<VersionFileMetadata>,
}

pub fn normalize_display_name(input: &str) -> String {
    input
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

pub fn build_folder_slug(kind: RepositoryEntityKind, human_name: &str, uuid: Uuid) -> String {
    let human_slug = slugify_name(human_name);
    format!("{human_slug}__{}_{}", kind.short_code(), short_uuid(uuid))
}

pub fn short_uuid(uuid: Uuid) -> String {
    uuid.simple().to_string()[..8].to_string()
}

pub fn slugify_name(input: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = false;

    for ch in input.trim().chars().flat_map(|c| c.to_lowercase()) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            last_was_dash = false;
            continue;
        }

        if !last_was_dash {
            slug.push('-');
            last_was_dash = true;
        }
    }

    let trimmed = slug.trim_matches('-');
    let mut normalized = if trimmed.is_empty() {
        "item".to_string()
    } else {
        trimmed.to_string()
    };

    if RESERVED_WINDOWS_NAMES.contains(&normalized.as_str()) {
        normalized = format!("x-{normalized}");
    }

    normalized
}

pub fn metadata_path(entity_dir: &Path) -> PathBuf {
    entity_dir.join(METADATA_FILE_NAME)
}

pub fn games_dir(repository_root: &Path) -> PathBuf {
    repository_root.join("games")
}

pub fn blobs_dir(repository_root: &Path) -> PathBuf {
    repository_root.join("blobs")
}

pub fn game_dir(repository_root: &Path, folder_slug: &str) -> PathBuf {
    games_dir(repository_root).join(folder_slug)
}

pub fn tracked_dirs_dir(game_dir: &Path) -> PathBuf {
    game_dir.join("tracked-dirs")
}

pub fn tracked_dir_dir(game_dir: &Path, folder_slug: &str) -> PathBuf {
    tracked_dirs_dir(game_dir).join(folder_slug)
}

pub fn slots_dir(game_dir: &Path) -> PathBuf {
    game_dir.join("slots")
}

pub fn slot_dir(game_dir: &Path, folder_slug: &str) -> PathBuf {
    slots_dir(game_dir).join(folder_slug)
}

pub fn versions_dir(slot_dir: &Path) -> PathBuf {
    slot_dir.join("versions")
}

pub fn version_dir(slot_dir: &Path, folder_slug: &str) -> PathBuf {
    versions_dir(slot_dir).join(folder_slug)
}

pub fn read_metadata_json<T>(entity_dir: &Path) -> Result<T>
where
    T: DeserializeOwned,
{
    let path = metadata_path(entity_dir);
    let file = File::open(&path)
        .with_context(|| format!("failed to open metadata file at {}", path.display()))?;
    let reader = BufReader::new(file);

    serde_json::from_reader(reader)
        .with_context(|| format!("failed to parse metadata file at {}", path.display()))
}

pub fn write_metadata_json<T>(entity_dir: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    fs::create_dir_all(entity_dir)
        .with_context(|| format!("failed to create metadata dir at {}", entity_dir.display()))?;

    let path = metadata_path(entity_dir);
    let tmp_path = metadata_tmp_path(&path);
    let payload = serde_json::to_vec_pretty(value).context("failed to serialize metadata")?;

    {
        let mut file = File::create(&tmp_path).with_context(|| {
            format!(
                "failed to create temp metadata file at {}",
                tmp_path.display()
            )
        })?;
        file.write_all(&payload).with_context(|| {
            format!(
                "failed to write temp metadata file at {}",
                tmp_path.display()
            )
        })?;
        file.write_all(b"\n").with_context(|| {
            format!(
                "failed to finalize temp metadata file at {}",
                tmp_path.display()
            )
        })?;
        file.sync_all().with_context(|| {
            format!(
                "failed to flush temp metadata file at {}",
                tmp_path.display()
            )
        })?;
    }

    sync_parent_dir(&tmp_path);

    atomic_replace(&tmp_path, &path).with_context(|| {
        format!(
            "failed to atomically move metadata into place from {} to {}",
            tmp_path.display(),
            path.display()
        )
    })?;

    sync_parent_dir(&path);
    Ok(())
}

#[cfg(windows)]
fn atomic_replace(source: &Path, destination: &Path) -> io::Result<()> {
    use std::iter;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn atomic_replace(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

pub fn ensure_repository_root(repository_root: &Path) -> Result<()> {
    let root_metadata_path = metadata_path(repository_root);
    if root_metadata_path.exists() {
        let metadata: RepositoryMetadata = read_metadata_json(repository_root)?;
        if metadata.format_version != REPOSITORY_FORMAT_VERSION {
            return Err(anyhow!(
                "repository format {} is unsupported; this pre-release build requires wiping and recreating the repository",
                metadata.format_version
            ));
        }
    }

    fs::create_dir_all(games_dir(repository_root)).with_context(|| {
        format!(
            "failed to create repository games dir at {}",
            games_dir(repository_root).display()
        )
    })?;
    fs::create_dir_all(blobs_dir(repository_root)).with_context(|| {
        format!(
            "failed to create repository blobs dir at {}",
            blobs_dir(repository_root).display()
        )
    })?;

    if !root_metadata_path.exists() {
        let metadata = RepositoryMetadata {
            format_version: REPOSITORY_FORMAT_VERSION,
            created_at: Utc::now(),
            app: "savegame-timeline".to_string(),
        };
        write_metadata_json(repository_root, &metadata)?;
    }

    Ok(())
}

pub fn sync_game_metadata(repository_root: &Path, game: &crate::models::FullGame) -> Result<()> {
    let game_uuid = parse_uuid_field(game.game.uuid.as_deref(), "game.uuid")?;
    let folder_slug = validate_folder_slug(game.game.folder_slug.as_deref(), "game.folder_slug")?;
    let game_entity_dir = game_dir(repository_root, folder_slug);
    let existing_game_metadata = read_metadata_json::<GameMetadata>(&game_entity_dir).ok();
    let now = Utc::now();

    let game_metadata = GameMetadata {
        entity: "game".to_string(),
        uuid: game_uuid,
        title: game.game.title.clone(),
        folder_slug: folder_slug.to_string(),
        active_slot_uuid: game
            .active_slot
            .as_ref()
            .and_then(|slot| Uuid::parse_str(&slot.uuid).ok()),
        icon: game.game.icon_path.clone(),
        created_at: existing_game_metadata
            .as_ref()
            .map(|metadata| metadata.created_at)
            .unwrap_or(now),
        updated_at: now,
    };
    write_metadata_json(&game_entity_dir, &game_metadata)?;

    let active_uuids = game
        .tracked_dirs
        .iter()
        .map(|dir| parse_uuid_field(dir.uuid.as_deref(), "tracked_dir.uuid"))
        .collect::<Result<std::collections::HashSet<_>>>()?;
    let tracked_root = tracked_dirs_dir(&game_entity_dir);
    if tracked_root.exists() {
        for entry in fs::read_dir(&tracked_root)? {
            let path = entry?.path();
            if !path.is_dir() {
                continue;
            }
            let mut metadata: TrackedDirMetadata = read_metadata_json(&path)?;
            if metadata.game_uuid != game_uuid {
                return Err(anyhow!(
                    "Tracked directory metadata belongs to another game"
                ));
            }
            if !active_uuids.contains(&metadata.uuid) && metadata.retired_at.is_none() {
                metadata.retired_at = Some(now);
                metadata.updated_at = now;
                write_metadata_json(&path, &metadata)?;
            }
        }
    }

    for tracked_dir in &game.tracked_dirs {
        let tracked_uuid = parse_uuid_field(tracked_dir.uuid.as_deref(), "tracked_dir.uuid")?;
        let tracked_slug = validate_folder_slug(
            tracked_dir.folder_slug.as_deref(),
            "tracked_dir.folder_slug",
        )?;
        let tracked_dir_entity_dir = tracked_dir_dir(&game_entity_dir, tracked_slug);
        let existing_metadata =
            read_metadata_json::<TrackedDirMetadata>(&tracked_dir_entity_dir).ok();

        let tracked_metadata = TrackedDirMetadata {
            entity: "tracked_dir".to_string(),
            uuid: tracked_uuid,
            game_uuid,
            path: crate::path_placeholders::substitute_path_placeholders(Path::new(
                &tracked_dir.path,
            )),
            label: None,
            exclusions: crate::exclusions::parse(&tracked_dir.exclusions)
                .iter()
                .map(|pattern| pattern.as_str().to_string())
                .collect(),
            folder_slug: tracked_slug.to_string(),
            retired_at: None,
            created_at: existing_metadata
                .as_ref()
                .map(|metadata| metadata.created_at)
                .unwrap_or(now),
            updated_at: now,
        };

        write_metadata_json(&tracked_dir_entity_dir, &tracked_metadata)?;
    }

    for slot in &game.slots {
        let slot_uuid = Uuid::parse_str(&slot.uuid).context("failed to parse slot.uuid")?;
        let slot_slug = validate_folder_slug(Some(&slot.folder_slug), "slot.folder_slug")?;
        let slot_entity_dir = slot_dir(&game_entity_dir, slot_slug);
        let existing_metadata = read_metadata_json::<SlotMetadata>(&slot_entity_dir).ok();
        let active_version_uuid = if game
            .active_slot
            .as_ref()
            .is_some_and(|active_slot| active_slot.id == slot.id)
        {
            game.last_version
                .as_ref()
                .and_then(|version| version.uuid.as_deref())
                .and_then(|uuid| Uuid::parse_str(uuid).ok())
        } else {
            existing_metadata
                .as_ref()
                .and_then(|metadata| metadata.active_version_uuid)
        };

        let slot_metadata = SlotMetadata {
            entity: "slot".to_string(),
            uuid: slot_uuid,
            game_uuid,
            name: slot.name.clone(),
            folder_slug: slot.folder_slug.clone(),
            active_version_uuid,
            created_at: existing_metadata
                .as_ref()
                .map(|metadata| metadata.created_at)
                .unwrap_or(DateTime::<Utc>::from_naive_utc_and_offset(
                    slot.created_at,
                    Utc,
                )),
            updated_at: now,
        };
        write_metadata_json(&slot_entity_dir, &slot_metadata)?;
    }

    Ok(())
}

pub fn sync_version_metadata(
    repository_root: &Path,
    version: &crate::models::FullVersion,
) -> Result<()> {
    let folder_slug = validate_folder_slug(
        version.version.folder_slug.as_deref(),
        "version.folder_slug",
    )?;
    let game_folder_slug =
        validate_folder_slug(version.game.folder_slug.as_deref(), "game.folder_slug")?;
    let slot_folder_slug =
        validate_folder_slug(Some(&version.slot.folder_slug), "slot.folder_slug")?;
    let game_entity_dir = game_dir(repository_root, game_folder_slug);
    let slot_entity_dir = slot_dir(&game_entity_dir, slot_folder_slug);
    let version_entity_dir = version_dir(&slot_entity_dir, folder_slug);
    let mut metadata: VersionMetadata =
        read_metadata_json(&version_entity_dir).with_context(|| {
            format!(
                "failed to read existing version metadata at {}",
                version_entity_dir.display()
            )
        })?;
    metadata.tag = version.version.tag.clone();
    write_metadata_json(&version_entity_dir, &metadata)
}

pub fn commit_version_metadata(
    repository_root: &Path,
    version: &crate::models::FullVersion,
    snapshot: &crate::content_store::CapturedSnapshot,
) -> Result<()> {
    let version_uuid = parse_uuid_field(version.version.uuid.as_deref(), "version.uuid")?;
    let game_uuid = parse_uuid_field(version.game.uuid.as_deref(), "game.uuid")?;
    let folder_slug = validate_folder_slug(
        version.version.folder_slug.as_deref(),
        "version.folder_slug",
    )?;
    let parent_version_uuid = version
        .parent_uuid
        .as_deref()
        .map(|value| Uuid::parse_str(value).context("failed to parse parent version uuid"))
        .transpose()?;
    let slot_uuid = Uuid::parse_str(&version.slot.uuid).context("failed to parse slot.uuid")?;
    let game_folder_slug =
        validate_folder_slug(version.game.folder_slug.as_deref(), "game.folder_slug")?;
    let slot_folder_slug =
        validate_folder_slug(Some(&version.slot.folder_slug), "slot.folder_slug")?;

    let metadata = VersionMetadata {
        entity: "version".to_string(),
        uuid: version_uuid,
        game_uuid,
        slot_uuid: Some(slot_uuid),
        version_number: version.version.version_number,
        folder_slug: folder_slug.to_string(),
        parent_version_uuid,
        tag: version.version.tag.clone(),
        reason: VersionReason::from_str_or_watcher(&version.version.reason),
        created_at: DateTime::<Utc>::from_naive_utc_and_offset(version.version.created_at, Utc),
        content_hash: snapshot.content_hash.clone(),
        files: snapshot.files.clone(),
    };

    let game_entity_dir = game_dir(repository_root, game_folder_slug);
    let slot_entity_dir = slot_dir(&game_entity_dir, slot_folder_slug);
    let versions_parent = versions_dir(&slot_entity_dir);
    let final_dir = version_dir(&slot_entity_dir, folder_slug);
    ensure_repository_entry_absent(&final_dir)?;

    fs::create_dir_all(&versions_parent).with_context(|| {
        format!(
            "failed to create versions directory at {}",
            versions_parent.display()
        )
    })?;
    let temp_dir = versions_parent.join(format!("{folder_slug}.tmp-{}", Uuid::new_v4()));
    fs::create_dir(&temp_dir).with_context(|| {
        format!(
            "failed to create temporary version directory at {}",
            temp_dir.display()
        )
    })?;

    let result = (|| -> Result<()> {
        write_metadata_json(&temp_dir, &metadata)?;
        sync_parent_dir(&temp_dir);
        match crate::content_store::atomic_rename_no_replace(&temp_dir, &final_dir) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(anyhow!(
                    "version metadata already exists at {}",
                    final_dir.display()
                ));
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "failed to publish version metadata from {} to {}",
                        temp_dir.display(),
                        final_dir.display()
                    )
                });
            }
        }
        sync_parent_dir(&final_dir);
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_dir_all(&temp_dir);
    }
    result
}

fn metadata_tmp_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(METADATA_FILE_NAME);
    let tmp_name = format!("{file_name}.tmp-{}", short_uuid(Uuid::new_v4()));
    path.with_file_name(tmp_name)
}

fn parse_uuid_field(value: Option<&str>, field_name: &str) -> Result<Uuid> {
    let value = required_string_field(value, field_name)?;
    Uuid::parse_str(value).with_context(|| format!("failed to parse UUID for {field_name}"))
}

fn validate_folder_slug<'a>(value: Option<&'a str>, field_name: &str) -> Result<&'a str> {
    let value = required_string_field(value, field_name)?;
    let path = Path::new(value);
    let mut components = path.components();
    let is_single_normal_component =
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none();
    if path.is_absolute()
        || path.has_root()
        || !is_single_normal_component
        || value == "."
        || value == ".."
        || value.contains('/')
        || value.contains('\\')
    {
        return Err(anyhow!(
            "invalid {field_name}: expected one normal path component"
        ));
    }
    Ok(value)
}

fn ensure_repository_entry_absent(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(anyhow!(
            "version metadata already exists at {}",
            path.display()
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| {
            format!(
                "failed to inspect version metadata path at {}",
                path.display()
            )
        }),
    }
}

fn required_string_field<'a>(value: Option<&'a str>, field_name: &str) -> Result<&'a str> {
    value
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("missing required field {field_name}"))
}

fn sync_parent_dir(path: &Path) {
    let Some(parent) = path.parent() else {
        return;
    };

    if let Ok(dir) = File::open(parent) {
        let _ = dir.sync_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content_store::CapturedSnapshot;
    use crate::models::{FullVersion, Game, Slot, Version};
    use chrono::NaiveDateTime;
    use tempfile::tempdir;

    fn full_version_fixture() -> FullVersion {
        FullVersion {
            version: Version {
                id: 7,
                uuid: Some("11111111-1111-1111-1111-111111111111".to_string()),
                folder_slug: Some("v007__v_11111111".to_string()),
                version_number: 7,
                slot_id: 5,
                tag: Some("before-boss".to_string()),
                reason: "manual".to_string(),
                created_at: NaiveDateTime::parse_from_str(
                    "2026-01-02 03:04:05",
                    "%Y-%m-%d %H:%M:%S",
                )
                .unwrap(),
                parent_id: Some(6),
            },
            game: Game {
                id: 3,
                uuid: Some("33333333-3333-3333-3333-333333333333".to_string()),
                title: "Hollow Knight".to_string(),
                title_normalized: Some("hollow knight".to_string()),
                folder_slug: Some("hollow-knight__g_33333333".to_string()),
                active_slot_id: Some(5),
                icon_path: None,
            },
            slot: Slot {
                id: 5,
                uuid: "55555555-5555-5555-5555-555555555555".to_string(),
                game_id: 3,
                name: "Default Slot".to_string(),
                name_normalized: "default slot".to_string(),
                folder_slug: "default-slot__s_55555555".to_string(),
                active_version_id: Some(7),
                created_at: NaiveDateTime::parse_from_str(
                    "2026-01-01 00:00:00",
                    "%Y-%m-%d %H:%M:%S",
                )
                .unwrap(),
                updated_at: NaiveDateTime::parse_from_str(
                    "2026-01-02 03:04:05",
                    "%Y-%m-%d %H:%M:%S",
                )
                .unwrap(),
            },
            parent_uuid: Some("66666666-6666-6666-6666-666666666666".to_string()),
        }
    }

    fn file_entry_fixture() -> VersionFileMetadata {
        VersionFileMetadata {
            tracked_dir_uuid: Uuid::parse_str("44444444-4444-4444-4444-444444444444").unwrap(),
            relative_path: "save.dat".to_string(),
            size: 9,
            sha256: "sha256:0000000000000000000000000000000000000000000000000000000000000000"
                .to_string(),
        }
    }

    fn expected_version_dir(root: &Path, _version: &FullVersion) -> PathBuf {
        root.join("games")
            .join("hollow-knight__g_33333333")
            .join("slots")
            .join("default-slot__s_55555555")
            .join("versions")
            .join("v007__v_11111111")
    }

    #[test]
    fn normalizes_display_names_for_uniqueness() {
        assert_eq!(
            normalize_display_name("  Hollow   Knight "),
            "hollow knight"
        );
    }

    #[test]
    fn slugifies_names_for_repository_paths() {
        assert_eq!(slugify_name("Hollow Knight"), "hollow-knight");
        assert_eq!(slugify_name("AUX"), "x-aux");
        assert_eq!(slugify_name("???"), "item");
    }

    #[test]
    fn builds_readable_folder_slugs() {
        let uuid =
            Uuid::parse_str("8f3a2c9d-1111-2222-3333-444444444444").expect("valid test uuid");
        let slug = build_folder_slug(RepositoryEntityKind::Game, "Hollow Knight", uuid);

        assert_eq!(slug, "hollow-knight__g_8f3a2c9d");
    }

    #[test]
    fn serializes_version_reason_as_snake_case() {
        let serialized =
            serde_json::to_string(&VersionReason::PreSlotCreate).expect("serialize reason");
        assert_eq!(serialized, "\"pre_slot_create\"");
    }

    #[test]
    fn builds_new_repository_layout_paths_consistently() {
        let root = Path::new("/repo");
        let game = game_dir(root, "hollow-knight__g_8f3a2c9d");
        let slot = slot_dir(&game, "default-slot__s_27b91a04");
        let version = version_dir(&slot, "v001__v_91ac4e2a");

        assert_eq!(
            version,
            Path::new(
                "/repo/games/hollow-knight__g_8f3a2c9d/slots/default-slot__s_27b91a04/versions/v001__v_91ac4e2a"
            )
        );
        assert_eq!(
            metadata_path(&game),
            Path::new("/repo/games/hollow-knight__g_8f3a2c9d/metadata.json")
        );
    }

    #[test]
    fn writes_and_reads_metadata_json_round_trip() {
        let dir = tempdir().expect("temp dir");
        let entity_dir = game_dir(dir.path(), "hollow-knight__g_8f3a2c9d");
        let now = Utc::now();
        let metadata = RepositoryMetadata {
            format_version: 1,
            created_at: now,
            app: "savegame-timeline".to_string(),
        };

        write_metadata_json(&entity_dir, &metadata).expect("write metadata");
        let loaded: RepositoryMetadata = read_metadata_json(&entity_dir).expect("read metadata");

        assert_eq!(loaded, metadata);
    }

    #[test]
    fn overwrites_existing_metadata_json() {
        let dir = tempdir().expect("temp dir");
        let entity_dir = version_dir(dir.path(), "v001__v_91ac4e2a");
        let first = RepositoryMetadata {
            format_version: 1,
            created_at: Utc::now(),
            app: "savegame-timeline".to_string(),
        };
        let second = RepositoryMetadata {
            format_version: 2,
            created_at: Utc::now(),
            app: "savegame-timeline-next".to_string(),
        };

        write_metadata_json(&entity_dir, &first).expect("write first metadata");
        write_metadata_json(&entity_dir, &second).expect("write second metadata");

        let loaded: RepositoryMetadata = read_metadata_json(&entity_dir).expect("read metadata");
        assert_eq!(loaded, second);
    }

    #[test]
    fn replaces_existing_metadata_without_a_delete_gap() {
        let dir = tempdir().expect("temp dir");
        let entity_dir = version_dir(dir.path(), "v001__v_91ac4e2a");
        let first = RepositoryMetadata {
            format_version: 1,
            created_at: Utc::now(),
            app: "savegame-timeline-first".to_string(),
        };
        let second = RepositoryMetadata {
            format_version: 2,
            created_at: Utc::now(),
            app: "savegame-timeline-second".to_string(),
        };

        write_metadata_json(&entity_dir, &first).expect("write first metadata");
        write_metadata_json(&entity_dir, &second).expect("write second metadata");

        let loaded: RepositoryMetadata = read_metadata_json(&entity_dir).expect("read metadata");
        assert_eq!(loaded, second);
        assert_ne!(loaded, first);
        assert!(fs::read_dir(&entity_dir).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("metadata.json.tmp-")
        }));
    }

    #[test]
    fn ensures_repository_root_bootstraps_metadata_and_games_dir() {
        let dir = tempdir().expect("temp dir");

        ensure_repository_root(dir.path()).expect("ensure repository root");

        assert!(games_dir(dir.path()).exists(), "games dir should exist");
        assert!(blobs_dir(dir.path()).exists(), "blobs dir should exist");
        let loaded: RepositoryMetadata =
            read_metadata_json(dir.path()).expect("read repository metadata");
        assert_eq!(loaded.format_version, REPOSITORY_FORMAT_VERSION);
        assert_eq!(loaded.app, "savegame-timeline");
    }

    #[test]
    fn rejects_unsupported_repository_format() {
        let dir = tempdir().expect("temp dir");
        let metadata = RepositoryMetadata {
            format_version: 1,
            created_at: Utc::now(),
            app: "savegame-timeline".to_string(),
        };
        write_metadata_json(dir.path(), &metadata).expect("write metadata");

        let error = ensure_repository_root(dir.path()).expect_err("format should be rejected");
        assert_eq!(
            error.to_string(),
            "repository format 1 is unsupported; this pre-release build requires wiping and recreating the repository"
        );
    }

    #[test]
    fn commits_version_metadata_by_directory_rename_without_legacy_dirs() {
        let root = tempdir().unwrap();
        let version = full_version_fixture();
        let snapshot = CapturedSnapshot {
            content_hash: "sha256:0000000000000000000000000000000000000000000000000000000000000000"
                .into(),
            files: vec![file_entry_fixture()],
        };

        commit_version_metadata(root.path(), &version, &snapshot).unwrap();
        let final_dir = expected_version_dir(root.path(), &version);

        assert_eq!(
            read_metadata_json::<VersionMetadata>(&final_dir)
                .unwrap()
                .files,
            snapshot.files
        );
        assert!(!final_dir.join("dirs").exists());
        assert!(
            fs::read_dir(final_dir.parent().unwrap())
                .unwrap()
                .all(|entry| {
                    !entry
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .contains(".tmp-")
                })
        );
    }

    #[test]
    fn rejects_non_component_repository_slugs() {
        let root = tempdir().unwrap();
        for invalid_slug in ["", ".", "..", "nested/path", r"nested\path"] {
            let mut version = full_version_fixture();
            version.game.folder_slug = Some(invalid_slug.to_string());
            let error = commit_version_metadata(
                root.path(),
                &version,
                &CapturedSnapshot {
                    content_hash:
                        "sha256:0000000000000000000000000000000000000000000000000000000000000000"
                            .to_string(),
                    files: Vec::new(),
                },
            )
            .expect_err("path-like game slug must be rejected");
            assert!(error.to_string().contains("game.folder_slug"));
        }
        assert!(validate_folder_slug(Some("C:\\absolute"), "game.folder_slug").is_err());
    }

    #[test]
    fn rejects_preexisting_final_version_entry_without_replacement() {
        let root = tempdir().unwrap();
        let version = full_version_fixture();
        let final_dir = expected_version_dir(root.path(), &version);
        fs::create_dir_all(&final_dir).unwrap();
        fs::write(final_dir.join("sentinel"), b"keep me").unwrap();

        let error = commit_version_metadata(
            root.path(),
            &version,
            &CapturedSnapshot {
                content_hash:
                    "sha256:0000000000000000000000000000000000000000000000000000000000000000"
                        .to_string(),
                files: Vec::new(),
            },
        )
        .expect_err("pre-existing final entries are immutable conflicts");
        assert!(error.to_string().contains("already exists"));
        assert_eq!(fs::read(final_dir.join("sentinel")).unwrap(), b"keep me");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_dangling_final_version_entry_without_replacement() {
        use std::os::unix::fs::symlink;

        let root = tempdir().unwrap();
        let version = full_version_fixture();
        let final_dir = expected_version_dir(root.path(), &version);
        fs::create_dir_all(final_dir.parent().unwrap()).unwrap();
        symlink("missing-version-target", &final_dir).unwrap();

        let error = commit_version_metadata(
            root.path(),
            &version,
            &CapturedSnapshot {
                content_hash:
                    "sha256:0000000000000000000000000000000000000000000000000000000000000000"
                        .to_string(),
                files: Vec::new(),
            },
        )
        .expect_err("dangling final entries are immutable conflicts");
        assert!(error.to_string().contains("already exists"));
        assert!(
            fs::symlink_metadata(&final_dir)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
}
