use crate::persistence::GameRepository;
use crate::repository::{self, GameMetadata, TrackedDirMetadata};
use crate::sync::{
    RemoteGame, RemoteGameMetadata, RemoteSlot, RemoteSlotMetadata, RemoteVersionMetadata,
};
use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

const BUNDLE_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoryTrackedDirectory {
    pub uuid: Uuid,
    pub label: Option<String>,
    pub source_path_hint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoryBundleSummary {
    pub game_uuid: Uuid,
    pub title: String,
    pub slot_count: usize,
    pub version_count: usize,
    pub tracked_dir_count: usize,
    pub blob_count: usize,
    pub retired_version_count: usize,
    pub active_tracked_dirs: Vec<HistoryTrackedDirectory>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoryTransferReport {
    pub game_uuid: Uuid,
    pub title: String,
    pub slot_count: usize,
    pub version_count: usize,
    pub tracked_dir_count: usize,
    pub blob_count: usize,
    pub retired_version_count: usize,
}

impl From<&HistoryBundleSummary> for HistoryTransferReport {
    fn from(summary: &HistoryBundleSummary) -> Self {
        Self {
            game_uuid: summary.game_uuid,
            title: summary.title.clone(),
            slot_count: summary.slot_count,
            version_count: summary.version_count,
            tracked_dir_count: summary.tracked_dir_count,
            blob_count: summary.blob_count,
            retired_version_count: summary.retired_version_count,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleManifest {
    format_version: u32,
    game_uuid: Uuid,
    exported_at: DateTime<Utc>,
    slot_count: usize,
    version_count: usize,
    tracked_dir_count: usize,
    blob_count: usize,
    retired_version_count: usize,
}

struct Bundle {
    root: PathBuf,
    game: RemoteGame,
    retired: HashMap<Uuid, crate::pruning::RetirementMarker>,
    blob_sizes: HashMap<String, u64>,
    summary: HistoryBundleSummary,
}

pub(crate) fn export_game(
    repository_root: &Path,
    games_repo: &dyn GameRepository,
    game_id: i32,
    destination: &Path,
) -> Result<HistoryTransferReport> {
    let full_game = games_repo.get_game(game_id)?;
    let uuid = Uuid::parse_str(
        full_game
            .game
            .uuid
            .as_deref()
            .context("Game has no repository UUID")?,
    )?;
    let slug = repository::validate_folder_slug(
        full_game.game.folder_slug.as_deref(),
        "game folder_slug",
    )?;
    let local_game = read_game_tree(repository_root, slug, false)?;
    if local_game.metadata.uuid != uuid {
        bail!("Game UUID does not match its repository metadata");
    }
    if local_game.metadata.title != full_game.game.title {
        bail!("Game title does not match its repository metadata");
    }
    validate_db_graph(games_repo, game_id, &full_game, &local_game, None)?;

    let versions = local_game
        .slots
        .iter()
        .flat_map(|slot| slot.versions.iter().map(|version| &version.version))
        .collect::<Vec<_>>();
    let version_ids = versions
        .iter()
        .map(|version| version.uuid)
        .collect::<HashSet<_>>();
    let all_markers = crate::pruning::read_markers(repository_root)?;
    let retired = all_markers
        .into_iter()
        .filter(|(version_uuid, _)| version_ids.contains(version_uuid))
        .collect::<HashMap<_, _>>();
    crate::pruning::validate_markers(&catalog_for(&local_game, retired.clone()))?;
    let blob_sizes = referenced_blobs(&local_game, &retired)?;

    let root = repository_root
        .canonicalize()
        .context("Failed to resolve repository root")?;
    let (destination, parent) = prepare_new_destination(&root, destination)?;
    let staging = create_staging_dir(
        &parent,
        destination.file_name().context("Invalid destination")?,
    )?;
    let result = (|| {
        repository::ensure_repository_root(&staging.path)?;
        fs::create_dir_all(staging.path.join("retired-versions"))?;
        for (hash, size) in &blob_sizes {
            crate::sync::copy_blob(repository_root, &staging.path, hash, *size)?;
        }
        write_game_tree(&staging.path, &local_game)?;
        for marker in retired.values() {
            crate::pruning::write_marker(&staging.path, marker)?;
        }
        let summary = summary_for(&local_game, blob_sizes.len(), retired.len());
        let manifest = BundleManifest {
            format_version: BUNDLE_FORMAT_VERSION,
            game_uuid: uuid,
            exported_at: Utc::now(),
            slot_count: summary.slot_count,
            version_count: summary.version_count,
            tracked_dir_count: summary.tracked_dir_count,
            blob_count: summary.blob_count,
            retired_version_count: summary.retired_version_count,
        };
        repository::write_json_file(&staging.path.join("export.json"), &manifest)?;
        load_bundle(&staging.path)?;
        crate::content_store::atomic_rename_no_replace(&staging.path, &destination)
            .with_context(|| format!("Failed to publish bundle at {}", destination.display()))?;
        Ok(HistoryTransferReport::from(&summary))
    })();
    if result.is_err() {
        staging.remove();
    }
    result
}

pub fn inspect(path: &Path) -> Result<HistoryBundleSummary> {
    Ok(load_bundle(path)?.summary)
}

pub(crate) fn import_game(
    repository_root: &Path,
    games_repo: &dyn GameRepository,
    bundle_path: &Path,
    mappings: &HashMap<Uuid, PathBuf>,
) -> Result<HistoryTransferReport> {
    let bundle = load_bundle(bundle_path)?;
    let repository = repository_root
        .canonicalize()
        .context("Failed to resolve local repository")?;
    if paths_overlap(&bundle.root, &repository) {
        bail!("History bundle and local repository must be separate directories");
    }
    let mut incoming = bundle.game.clone();
    let existing_games = games_repo.get_games()?;
    let new_game_tree = merge_and_map_existing(
        games_repo,
        &existing_games,
        repository_root,
        &mut incoming,
        mappings,
        &bundle.retired,
    )?;
    validate_mapped_paths(&repository, &bundle.root, &incoming, &existing_games)?;

    // Verify every referenced source blob before publishing repository metadata or touching SQLite.
    for (hash, size) in &bundle.blob_sizes {
        crate::content_store::verify_blob(&bundle.root, hash, *size)?;
    }
    for (hash, size) in &bundle.blob_sizes {
        crate::sync::copy_blob(&bundle.root, repository_root, hash, *size)?;
    }
    let pulled = if new_game_tree {
        publish_new_game_tree(repository_root, &incoming)?;
        incoming.clone()
    } else {
        crate::sync::pull_game_history(repository_root, &bundle.root, &incoming, &bundle.retired)?
    };
    let expected = incoming
        .slots
        .iter()
        .map(|slot| slot.versions.len())
        .sum::<usize>();
    let actual = pulled
        .slots
        .iter()
        .map(|slot| slot.versions.len())
        .sum::<usize>();
    if actual != expected {
        bail!("History import copied {actual} of {expected} versions");
    }
    for marker in bundle.retired.values() {
        crate::pruning::write_marker(repository_root, marker)?;
    }
    games_repo.import_remote_game(&pulled, None)?;
    Ok(HistoryTransferReport::from(&bundle.summary))
}

fn merge_and_map_existing(
    games_repo: &dyn GameRepository,
    existing_games: &[crate::models::FullGame],
    repository_root: &Path,
    incoming: &mut RemoteGame,
    mappings: &HashMap<Uuid, PathBuf>,
    retired: &HashMap<Uuid, crate::pruning::RetirementMarker>,
) -> Result<bool> {
    let matching = existing_games
        .iter()
        .filter(|game| game.game.uuid.as_deref() == Some(&incoming.metadata.uuid.to_string()))
        .collect::<Vec<_>>();
    if matching.len() > 1 {
        bail!("Duplicate local game UUID {}", incoming.metadata.uuid);
    }
    let db_game = matching.first().copied();
    let slug =
        repository::validate_folder_slug(Some(&incoming.metadata.folder_slug), "bundle game slug")?;
    let local_dir = repository::game_dir(repository_root, slug);
    let local_game = match fs::symlink_metadata(&local_dir) {
        Ok(_) => Some(read_game_tree(repository_root, slug, false)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };

    if let Some(game) = db_game {
        if game.game.folder_slug.as_deref() != Some(slug) {
            bail!(
                "Game UUID {} already uses a different folder slug",
                incoming.metadata.uuid
            );
        }
        let local = local_game
            .as_ref()
            .context("Game exists in SQLite but its repository metadata is missing")?;
        validate_db_graph(games_repo, game.game.id, game, local, Some(incoming))?;
    }
    if let Some(local) = &local_game {
        compare_game_identity(local, incoming)?;
    }
    let existing_markers = crate::pruning::read_markers(repository_root)?;
    for (uuid, marker) in retired {
        if let Some(existing) = existing_markers.get(uuid)
            && (existing.slot_uuid != marker.slot_uuid
                || existing.content_hash != marker.content_hash)
        {
            bail!("Conflicting retirement marker for version {uuid}");
        }
    }
    if let Some(other_slug) = find_game_slug(repository_root, incoming.metadata.uuid, slug)? {
        bail!(
            "Game UUID {} already exists under folder slug {other_slug}",
            incoming.metadata.uuid
        );
    }

    let local_tracks = local_game
        .as_ref()
        .map(|game| {
            game.tracked_dirs
                .iter()
                .map(|tracked| (tracked.uuid, tracked.path.clone()))
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();
    let db_tracks = db_game
        .map(|game| {
            game.tracked_dirs
                .iter()
                .filter_map(|tracked| {
                    Uuid::parse_str(tracked.uuid.as_deref()?)
                        .ok()
                        .map(|uuid| (uuid, tracked.path.clone()))
                })
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();
    let active_ids = incoming
        .tracked_dirs
        .iter()
        .filter(|tracked| tracked.retired_at.is_none())
        .map(|tracked| tracked.uuid)
        .collect::<HashSet<_>>();
    for uuid in mappings.keys() {
        if !active_ids.contains(uuid) {
            bail!("Mapping {uuid} does not name an active tracked directory in the bundle");
        }
    }

    for tracked in &mut incoming.tracked_dirs {
        if let Some(local) = local_game.as_ref().and_then(|game| {
            game.tracked_dirs
                .iter()
                .find(|local| local.uuid == tracked.uuid)
        }) {
            compare_tracked_identity(local, tracked)?;
        } else if let Some(local) = local_game.as_ref().and_then(|game| {
            game.tracked_dirs
                .iter()
                .find(|local| local.folder_slug == tracked.folder_slug)
        }) {
            bail!(
                "Tracked-directory folder slug {} belongs to UUID {}",
                tracked.folder_slug,
                local.uuid
            );
        }
        let existing_path = db_tracks
            .get(&tracked.uuid)
            .or_else(|| local_tracks.get(&tracked.uuid));
        if let Some(path) = existing_path {
            tracked.path = path.clone();
        } else if tracked.retired_at.is_some() {
            // A retired definition has no target location and must not inherit a source-machine path.
            tracked.path = retired_tracked_dir_path(incoming.metadata.uuid, tracked.uuid);
        } else {
            let path = mappings.get(&tracked.uuid).ok_or_else(|| {
                anyhow!(
                    "Missing destination mappings for tracked directory UUIDs: {}",
                    tracked.uuid
                )
            })?;
            validate_target_path(path)?;
            tracked.path = path
                .to_str()
                .ok_or_else(|| anyhow!("Mapped directory path is not valid UTF-8"))?
                .to_string();
        }
    }

    if let Some(local) = &local_game {
        merge_local_tags(local, incoming)?;
    }
    Ok(local_game.is_none())
}

fn compare_game_identity(local: &RemoteGame, incoming: &RemoteGame) -> Result<()> {
    if local.metadata.uuid != incoming.metadata.uuid
        || local.metadata.folder_slug != incoming.metadata.folder_slug
        || local.metadata.entity != incoming.metadata.entity
        || local.metadata.created_at != incoming.metadata.created_at
    {
        bail!("Game UUID was reused with different immutable metadata");
    }
    for slot in &incoming.slots {
        if let Some(existing) = local
            .slots
            .iter()
            .find(|candidate| candidate.metadata.uuid == slot.metadata.uuid)
        {
            if existing.metadata.game_uuid != slot.metadata.game_uuid
                || existing.metadata.folder_slug != slot.metadata.folder_slug
                || existing.metadata.entity != slot.metadata.entity
                || existing.metadata.created_at != slot.metadata.created_at
            {
                bail!(
                    "Slot UUID {} was reused with different immutable metadata",
                    slot.metadata.uuid
                );
            }
        } else if let Some(existing) = local
            .slots
            .iter()
            .find(|candidate| candidate.metadata.folder_slug == slot.metadata.folder_slug)
        {
            bail!(
                "Slot folder slug {} belongs to UUID {}",
                slot.metadata.folder_slug,
                existing.metadata.uuid
            );
        }
        for version in &slot.versions {
            if let Some(existing) = local
                .slots
                .iter()
                .flat_map(|candidate| &candidate.versions)
                .find(|candidate| candidate.version.uuid == version.version.uuid)
            {
                let mut left = existing.version.clone();
                let mut right = version.version.clone();
                left.tag = None;
                right.tag = None;
                if left != right {
                    bail!(
                        "Version UUID {} was reused with different metadata",
                        version.version.uuid
                    );
                }
            } else if let Some(existing) = local
                .slots
                .iter()
                .filter(|candidate| candidate.metadata.uuid == slot.metadata.uuid)
                .flat_map(|candidate| &candidate.versions)
                .find(|candidate| candidate.version.folder_slug == version.version.folder_slug)
            {
                bail!(
                    "Version folder slug {} belongs to UUID {}",
                    version.version.folder_slug,
                    existing.version.uuid
                );
            }
        }
    }
    Ok(())
}

fn compare_tracked_identity(
    local: &TrackedDirMetadata,
    incoming: &TrackedDirMetadata,
) -> Result<()> {
    if local.uuid != incoming.uuid
        || local.game_uuid != incoming.game_uuid
        || local.folder_slug != incoming.folder_slug
        || local.entity != incoming.entity
        || local.created_at != incoming.created_at
    {
        bail!(
            "Tracked-directory UUID {} was reused with different immutable metadata",
            incoming.uuid
        );
    }
    Ok(())
}

fn merge_local_tags(local: &RemoteGame, incoming: &mut RemoteGame) -> Result<()> {
    for slot in &mut incoming.slots {
        for version in &mut slot.versions {
            if let Some(existing) = local
                .slots
                .iter()
                .flat_map(|candidate| &candidate.versions)
                .find(|candidate| candidate.version.uuid == version.version.uuid)
                && existing
                    .version
                    .tag
                    .as_deref()
                    .is_some_and(|tag| !tag.trim().is_empty())
            {
                version.version.tag = existing.version.tag.clone();
            }
        }
    }
    Ok(())
}

fn validate_mapped_paths(
    repository: &Path,
    bundle: &Path,
    game: &RemoteGame,
    existing_games: &[crate::models::FullGame],
) -> Result<()> {
    let mut used = HashSet::new();
    let game_uuid = game.metadata.uuid.to_string();
    for tracked in game
        .tracked_dirs
        .iter()
        .filter(|tracked| tracked.retired_at.is_none())
    {
        let path = Path::new(&tracked.path);
        validate_target_path(path)?;
        let resolved = resolve_path(path)?;
        if paths_overlap(&resolved, repository) || paths_overlap(&resolved, bundle) {
            bail!(
                "Tracked directory path overlaps the repository or bundle: {}",
                path.display()
            );
        }
        if !used.insert(resolved.clone()) {
            bail!(
                "Two active tracked directories map to the same path: {}",
                path.display()
            );
        }
        for existing_game in existing_games {
            for existing_tracked in &existing_game.tracked_dirs {
                let same_directory = existing_game.game.uuid.as_deref() == Some(game_uuid.as_str())
                    && existing_tracked
                        .uuid
                        .as_deref()
                        .and_then(|uuid| Uuid::parse_str(uuid).ok())
                        == Some(tracked.uuid);
                if same_directory {
                    continue;
                }
                let existing_path =
                    crate::path_placeholders::expand_path_placeholders(&existing_tracked.path);
                if resolve_path(Path::new(&existing_path))? == resolved {
                    bail!(
                        "Tracked directory path is already used by another game: {}",
                        path.display()
                    );
                }
            }
        }
    }
    Ok(())
}

fn validate_target_path(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        bail!(
            "Tracked directory mapping must be an absolute path without '..': {}",
            path.display()
        );
    }
    crate::fs_utils::validate_restore_path(path, Path::new(""))
        .with_context(|| format!("Unsafe tracked directory mapping {}", path.display()))
}

fn resolve_path(path: &Path) -> Result<PathBuf> {
    let mut missing = Vec::new();
    let mut current = path;
    loop {
        match fs::symlink_metadata(current) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let name = current.file_name().context("Invalid path")?;
                missing.push(name.to_os_string());
                current = current.parent().context("Path has no existing ancestor")?;
            }
            Err(error) => return Err(error.into()),
        }
    }
    let metadata = fs::symlink_metadata(current)?;
    if !metadata.is_dir() || crate::fs_utils::is_link_or_reparse(&metadata) {
        bail!(
            "Tracked directory path has an unsafe existing ancestor: {}",
            current.display()
        );
    }
    let mut resolved = current.canonicalize()?;
    for component in missing.into_iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

fn prepare_new_destination(repository: &Path, destination: &Path) -> Result<(PathBuf, PathBuf)> {
    let absolute = if destination.is_absolute() {
        destination.to_path_buf()
    } else {
        std::env::current_dir()?.join(destination)
    };
    let name = absolute
        .file_name()
        .context("Destination must name a new directory")?;
    if name == "." || name == ".." {
        bail!("Destination must name a new directory");
    }
    let parent = absolute
        .parent()
        .context("Destination has no parent")?
        .canonicalize()
        .context("Destination parent must already exist")?;
    let destination = parent.join(name);
    if paths_overlap(&destination, repository) {
        bail!("Export destination must be outside the live repository");
    }
    match fs::symlink_metadata(&destination) {
        Ok(_) => bail!(
            "Export destination already exists: {}",
            destination.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok((destination, parent))
}

struct StagingDir {
    path: PathBuf,
}

impl StagingDir {
    fn remove(&self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn create_staging_dir(parent: &Path, target_name: &std::ffi::OsStr) -> Result<StagingDir> {
    for _ in 0..8 {
        let path = parent.join(format!(
            ".{}.history-{}",
            target_name.to_string_lossy(),
            Uuid::new_v4()
        ));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(StagingDir { path }),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error).context("Failed to create history staging directory"),
        }
    }
    bail!("Could not allocate a unique export staging directory")
}

fn load_bundle(path: &Path) -> Result<Bundle> {
    let metadata = fs::symlink_metadata(path).context("History bundle directory is unavailable")?;
    if !metadata.is_dir() || crate::fs_utils::is_link_or_reparse(&metadata) {
        bail!("History bundle is not a regular directory");
    }
    let root = path.canonicalize()?;
    require_exact_names(
        &root,
        &[
            "blobs",
            "export.json",
            "games",
            "metadata.json",
            "retired-versions",
        ],
    )?;
    require_directory(&root.join("games"))?;
    require_directory(&root.join("blobs"))?;
    require_directory(&root.join("retired-versions"))?;
    require_regular_file(&root.join("metadata.json"))?;
    require_regular_file(&root.join("export.json"))?;

    let repository_meta: repository::RepositoryMetadata = repository::read_metadata_json(&root)?;
    if repository_meta.format_version != repository::REPOSITORY_FORMAT_VERSION {
        bail!(
            "Unsupported repository format {}",
            repository_meta.format_version
        );
    }
    let manifest: BundleManifest = repository::read_json_file(&root.join("export.json"))?;
    if manifest.format_version != BUNDLE_FORMAT_VERSION {
        bail!(
            "Unsupported history bundle format {}",
            manifest.format_version
        );
    }
    let game_dirs = directory_children(&root.join("games"))?;
    if game_dirs.len() != 1 {
        bail!("History bundle must contain exactly one game subtree");
    }
    let game_name = game_dirs[0]
        .file_name()
        .and_then(|name| name.to_str())
        .context("Invalid game folder name")?;
    let game = read_game_dir(&game_dirs[0], true)?;
    if game.metadata.folder_slug != game_name || game.metadata.uuid != manifest.game_uuid {
        bail!("History bundle manifest does not match game metadata");
    }
    let all_versions = game
        .slots
        .iter()
        .flat_map(|slot| &slot.versions)
        .map(|v| v.version.uuid)
        .collect::<HashSet<_>>();
    let retired = read_bundle_markers(&root)?;
    if retired.keys().any(|uuid| !all_versions.contains(uuid)) {
        bail!("History bundle has a retirement marker for a different game");
    }
    crate::pruning::validate_markers(&catalog_for(&game, retired.clone()))?;
    let blob_sizes = referenced_blobs(&game, &retired)?;
    validate_blob_tree(&root, &blob_sizes)?;

    let report = crate::integrity::check_repository(&root, true);
    if !report.issues.is_empty() {
        let issues = report
            .issues
            .iter()
            .map(|issue| {
                format!(
                    "{}: {:?}: {}",
                    issue.path.display(),
                    issue.kind,
                    issue.detail
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        bail!("History bundle failed deep integrity validation: {issues}");
    }
    let summary = summary_for(&game, blob_sizes.len(), retired.len());
    if summary.slot_count != manifest.slot_count
        || summary.version_count != manifest.version_count
        || summary.tracked_dir_count != manifest.tracked_dir_count
        || summary.blob_count != manifest.blob_count
        || summary.retired_version_count != manifest.retired_version_count
    {
        bail!("History bundle manifest counts do not match its contents");
    }
    Ok(Bundle {
        root,
        game,
        retired,
        blob_sizes,
        summary,
    })
}

fn read_game_tree(root: &Path, slug: &str, bundle: bool) -> Result<RemoteGame> {
    let game_dir = repository::game_dir(root, slug);
    require_directory(&game_dir)?;
    read_game_dir(&game_dir, bundle)
}

fn read_game_dir(game_dir: &Path, bundle: bool) -> Result<RemoteGame> {
    require_directory(game_dir)?;
    require_exact_names(game_dir, &["metadata.json", "slots", "tracked-dirs"])?;
    let metadata: RemoteGameMetadata = read_metadata(game_dir)?;
    require_entity(&metadata.entity, "game")?;
    let slug = game_dir
        .file_name()
        .and_then(|name| name.to_str())
        .context("Invalid game folder name")?;
    repository::validate_folder_slug(Some(&metadata.folder_slug), "game folder_slug")?;
    if metadata.folder_slug != slug {
        bail!("Game folder slug does not match its directory name");
    }
    let mut tracked_dirs = Vec::new();
    let mut tracked_uuids = HashSet::new();
    for dir in directory_children(&repository::tracked_dirs_dir(game_dir))? {
        require_exact_names(&dir, &["metadata.json"])?;
        let tracked: TrackedDirMetadata = read_metadata(&dir)?;
        require_entity(&tracked.entity, "tracked_dir")?;
        check_slug(&tracked.folder_slug, &dir, "tracked directory")?;
        if tracked.game_uuid != metadata.uuid || !tracked_uuids.insert(tracked.uuid) {
            bail!("Tracked directory identity mismatch or duplicate UUID");
        }
        tracked_dirs.push(tracked);
    }
    let mut slots = Vec::new();
    let mut slot_uuids = HashSet::new();
    let mut version_uuids = HashSet::new();
    for dir in directory_children(&repository::slots_dir(game_dir))? {
        let mut names = vec!["metadata.json"];
        if bundle || dir.join("versions").exists() {
            names.push("versions");
        }
        require_exact_names(&dir, &names)?;
        let slot: RemoteSlotMetadata = read_metadata(&dir)?;
        require_entity(&slot.entity, "slot")?;
        check_slug(&slot.folder_slug, &dir, "slot")?;
        if slot.game_uuid != metadata.uuid || !slot_uuids.insert(slot.uuid) {
            bail!("Slot identity mismatch or duplicate UUID");
        }
        let versions_dir = repository::versions_dir(&dir);
        let mut versions = Vec::new();
        if versions_dir.exists() {
            require_directory(&versions_dir)?;
            for version_dir in directory_children(&versions_dir)? {
                require_exact_names(&version_dir, &["metadata.json"])?;
                let version: RemoteVersionMetadata = read_metadata(&version_dir)?;
                require_entity(&version.version.entity, "version")?;
                check_slug(&version.version.folder_slug, &version_dir, "version")?;
                if version.version.game_uuid != metadata.uuid
                    || version.version.slot_uuid != Some(slot.uuid)
                    || !version_uuids.insert(version.version.uuid)
                {
                    bail!("Version identity mismatch or duplicate UUID");
                }
                versions.push(version);
            }
        } else if bundle {
            bail!("Bundle slot is missing its versions directory");
        }
        versions.sort_by_key(|version| version.version.version_number);
        slots.push(RemoteSlot {
            metadata: slot,
            versions,
        });
    }
    tracked_dirs.sort_by_key(|tracked| tracked.uuid);
    slots.sort_by_key(|slot| slot.metadata.uuid);
    let game = RemoteGame {
        metadata,
        tracked_dirs,
        slots,
    };
    validate_graph(&game)?;
    Ok(game)
}

fn validate_graph(game: &RemoteGame) -> Result<()> {
    let tracks = game
        .tracked_dirs
        .iter()
        .map(|tracked| tracked.uuid)
        .collect::<HashSet<_>>();
    let mut versions = HashMap::new();
    for slot in &game.slots {
        for version in &slot.versions {
            let metadata = &version.version;
            for file in &metadata.files {
                if !tracks.contains(&file.tracked_dir_uuid) {
                    bail!(
                        "Version {} references an unknown tracked directory",
                        metadata.uuid
                    );
                }
            }
            if versions.insert(metadata.uuid, slot.metadata.uuid).is_some() {
                bail!("Duplicate version UUID {}", metadata.uuid);
            }
        }
    }
    for slot in &game.slots {
        for version in &slot.versions {
            if let Some(parent) = version.version.parent_version_uuid
                && (versions.get(&parent) != Some(&slot.metadata.uuid)
                    || parent == version.version.uuid)
            {
                bail!(
                    "Version {} has a missing or cross-slot parent",
                    version.version.uuid
                );
            }
        }
    }
    let parents = game
        .slots
        .iter()
        .flat_map(|slot| &slot.versions)
        .map(|version| (version.version.uuid, version.version.parent_version_uuid))
        .collect::<HashMap<_, _>>();
    let mut states = HashMap::new();
    for version_uuid in parents.keys() {
        let mut path = Vec::new();
        let mut current = Some(*version_uuid);
        while let Some(uuid) = current {
            match states.get(&uuid) {
                Some(1) => bail!("History bundle contains a version parent cycle"),
                Some(2) => break,
                _ => {}
            }
            states.insert(uuid, 1);
            path.push(uuid);
            current = parents.get(&uuid).copied().flatten();
        }
        for uuid in path {
            states.insert(uuid, 2);
        }
    }
    Ok(())
}

fn write_game_tree(root: &Path, game: &RemoteGame) -> Result<()> {
    let game_dir = repository::game_dir(root, &game.metadata.folder_slug);
    write_game_tree_dir(&game_dir, game)
}

fn write_game_tree_dir(game_dir: &Path, game: &RemoteGame) -> Result<()> {
    repository::write_metadata_json(game_dir, &game.metadata)?;
    fs::create_dir_all(repository::tracked_dirs_dir(game_dir))?;
    fs::create_dir_all(repository::slots_dir(game_dir))?;
    for tracked in &game.tracked_dirs {
        repository::write_metadata_json(
            &repository::tracked_dir_dir(game_dir, &tracked.folder_slug),
            tracked,
        )?;
    }
    for slot in &game.slots {
        let slot_dir = repository::slot_dir(game_dir, &slot.metadata.folder_slug);
        repository::write_metadata_json(&slot_dir, &slot.metadata)?;
        fs::create_dir_all(repository::versions_dir(&slot_dir))?;
        for version in &slot.versions {
            repository::write_metadata_json(
                &repository::version_dir(&slot_dir, &version.version.folder_slug),
                &RemoteVersionMetadata {
                    version: version.version.clone(),
                    origin_device_uuid: Uuid::nil(),
                },
            )?;
        }
    }
    Ok(())
}

fn publish_new_game_tree(repository_root: &Path, game: &RemoteGame) -> Result<()> {
    let games_root = repository::games_dir(repository_root);
    fs::create_dir_all(&games_root)?;
    let destination = repository::game_dir(repository_root, &game.metadata.folder_slug);
    let staging = create_staging_dir(
        &games_root,
        std::ffi::OsStr::new(&game.metadata.folder_slug),
    )?;
    let result = (|| {
        write_game_tree_dir(&staging.path, game)?;
        crate::content_store::atomic_rename_no_replace(&staging.path, &destination)
            .with_context(|| format!("Failed to publish imported game {}", game.metadata.uuid))?;
        Ok(())
    })();
    if result.is_err() {
        staging.remove();
    }
    result
}

fn referenced_blobs(
    game: &RemoteGame,
    retired: &HashMap<Uuid, crate::pruning::RetirementMarker>,
) -> Result<HashMap<String, u64>> {
    let mut blobs = HashMap::new();
    for version in game.slots.iter().flat_map(|slot| &slot.versions) {
        if retired.contains_key(&version.version.uuid) {
            continue;
        }
        for file in &version.version.files {
            crate::content_store::blob_path(Path::new("."), &file.sha256)?;
            if let Some(size) = blobs.insert(file.sha256.clone(), file.size)
                && size != file.size
            {
                bail!("Blob {} is referenced with conflicting sizes", file.sha256);
            }
        }
    }
    Ok(blobs)
}

fn validate_blob_tree(root: &Path, expected: &HashMap<String, u64>) -> Result<()> {
    let blob_root = repository::blobs_dir(root);
    let mut found = HashSet::new();
    for shard in directory_children(&blob_root)? {
        let shard_name = shard
            .file_name()
            .and_then(|name| name.to_str())
            .context("Invalid blob shard")?;
        if shard_name.len() != 2
            || !shard_name
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            bail!("Invalid blob shard {shard_name}");
        }
        for entry in regular_file_children(&shard)? {
            let name = entry
                .file_name()
                .and_then(|name| name.to_str())
                .context("Invalid blob name")?;
            let hash = format!("sha256:{shard_name}{name}");
            if crate::content_store::blob_path(root, &hash)? != entry || !found.insert(hash.clone())
            {
                bail!("Invalid or duplicate blob path {}", entry.display());
            }
        }
    }
    if found.len() != expected.len() || expected.keys().any(|hash| !found.contains(hash)) {
        bail!("History bundle blobs do not exactly match its non-retired versions");
    }
    for (hash, size) in expected {
        crate::content_store::verify_blob(root, hash, *size)?;
    }
    Ok(())
}

fn read_bundle_markers(root: &Path) -> Result<HashMap<Uuid, crate::pruning::RetirementMarker>> {
    let dir = root.join("retired-versions");
    let mut markers = HashMap::new();
    for entry in regular_file_children(&dir)? {
        let name = entry
            .file_name()
            .and_then(|name| name.to_str())
            .context("Invalid marker filename")?;
        let uuid = name
            .strip_suffix(".json")
            .and_then(|name| Uuid::parse_str(name).ok())
            .context("Invalid retirement marker filename")?;
        let marker: crate::pruning::RetirementMarker = repository::read_json_file(&entry)?;
        if marker.version_uuid != uuid || markers.insert(uuid, marker).is_some() {
            bail!("Retirement marker identity mismatch");
        }
    }
    Ok(markers)
}

fn catalog_for(
    game: &RemoteGame,
    retired: HashMap<Uuid, crate::pruning::RetirementMarker>,
) -> crate::sync::RemoteCatalog {
    crate::sync::RemoteCatalog {
        devices: Vec::new(),
        states: Vec::new(),
        games: vec![game.clone()],
        retired_versions: retired,
    }
}

fn summary_for(game: &RemoteGame, blob_count: usize, retired_count: usize) -> HistoryBundleSummary {
    HistoryBundleSummary {
        game_uuid: game.metadata.uuid,
        title: game.metadata.title.clone(),
        slot_count: game.slots.len(),
        version_count: game.slots.iter().map(|slot| slot.versions.len()).sum(),
        tracked_dir_count: game.tracked_dirs.len(),
        blob_count,
        retired_version_count: retired_count,
        active_tracked_dirs: game
            .tracked_dirs
            .iter()
            .filter(|tracked| tracked.retired_at.is_none())
            .map(|tracked| HistoryTrackedDirectory {
                uuid: tracked.uuid,
                label: tracked.label.clone(),
                source_path_hint: tracked.path.clone(),
            })
            .collect(),
    }
}

fn retired_tracked_dir_path(game_uuid: Uuid, tracked_uuid: Uuid) -> String {
    format!("retired-tracked-dir:{game_uuid}:{tracked_uuid}")
}

fn validate_db_graph(
    games_repo: &dyn GameRepository,
    game_id: i32,
    db_game: &crate::models::FullGame,
    game: &RemoteGame,
    pending: Option<&RemoteGame>,
) -> Result<()> {
    let slots = games_repo.list_slots(game_id)?;
    let db_slots = slots
        .iter()
        .map(|slot| Uuid::parse_str(&slot.slot.uuid))
        .collect::<std::result::Result<HashSet<_>, _>>()?;
    let repo_slots = game
        .slots
        .iter()
        .map(|slot| slot.metadata.uuid)
        .collect::<HashSet<_>>();
    let pending_slots = pending.map(|game| {
        game.slots
            .iter()
            .map(|slot| slot.metadata.uuid)
            .collect::<HashSet<_>>()
    });
    validate_uuid_set(&db_slots, &repo_slots, pending_slots.as_ref(), "slots")?;
    let db_tracks = db_game
        .tracked_dirs
        .iter()
        .filter_map(|tracked| tracked.uuid.as_deref())
        .map(Uuid::parse_str)
        .collect::<std::result::Result<HashSet<_>, _>>()?;
    let repo_tracks = game
        .tracked_dirs
        .iter()
        .filter(|tracked| tracked.retired_at.is_none())
        .map(|tracked| tracked.uuid)
        .collect::<HashSet<_>>();
    let pending_tracks = pending.map(|game| {
        game.tracked_dirs
            .iter()
            .filter(|tracked| tracked.retired_at.is_none())
            .map(|tracked| tracked.uuid)
            .collect::<HashSet<_>>()
    });
    validate_uuid_set(
        &db_tracks,
        &repo_tracks,
        pending_tracks.as_ref(),
        "tracked directories",
    )?;

    for slot in slots {
        let uuid = Uuid::parse_str(&slot.slot.uuid)?;
        let repo_slot = game
            .slots
            .iter()
            .find(|candidate| candidate.metadata.uuid == uuid)
            .context("Repository slot is missing")?;
        let db_versions = slot
            .versions
            .iter()
            .filter_map(|version| version.uuid.as_deref())
            .map(Uuid::parse_str)
            .collect::<std::result::Result<HashSet<_>, _>>()?;
        let repo_versions = repo_slot
            .versions
            .iter()
            .map(|version| version.version.uuid)
            .collect::<HashSet<_>>();
        let pending_versions = pending.and_then(|pending| {
            pending
                .slots
                .iter()
                .find(|candidate| candidate.metadata.uuid == uuid)
                .map(|slot| {
                    slot.versions
                        .iter()
                        .map(|version| version.version.uuid)
                        .collect::<HashSet<_>>()
                })
        });
        validate_uuid_set(
            &db_versions,
            &repo_versions,
            pending_versions.as_ref(),
            "versions",
        )?;
    }
    Ok(())
}

fn validate_uuid_set(
    database: &HashSet<Uuid>,
    repository: &HashSet<Uuid>,
    pending: Option<&HashSet<Uuid>>,
    kind: &str,
) -> Result<()> {
    let matches = match pending {
        None => database == repository,
        Some(pending) => {
            database.is_subset(repository)
                && repository
                    .difference(database)
                    .all(|uuid| pending.contains(uuid))
        }
    };
    if !matches {
        bail!("SQLite {kind} do not match repository history");
    }
    Ok(())
}

fn find_game_slug(root: &Path, uuid: Uuid, expected_slug: &str) -> Result<Option<String>> {
    for dir in directory_children(&repository::games_dir(root))? {
        let name = dir
            .file_name()
            .and_then(|name| name.to_str())
            .context("Invalid game folder")?;
        if name == expected_slug {
            continue;
        }
        let metadata_path = repository::metadata_path(&dir);
        if !metadata_path.exists() {
            continue;
        }
        let metadata: GameMetadata = read_metadata(&dir)?;
        if metadata.uuid == uuid {
            return Ok(Some(name.to_string()));
        }
    }
    Ok(None)
}

fn check_slug(slug: &str, directory: &Path, kind: &str) -> Result<()> {
    repository::validate_folder_slug(Some(slug), kind)?;
    if directory.file_name().and_then(|name| name.to_str()) != Some(slug) {
        bail!("{kind} folder slug does not match its directory name");
    }
    Ok(())
}

fn require_entity(value: &str, expected: &str) -> Result<()> {
    if value != expected {
        bail!("Expected {expected} metadata entity, got {value}");
    }
    Ok(())
}

fn require_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("Missing directory {}", path.display()))?;
    if !metadata.is_dir() || crate::fs_utils::is_link_or_reparse(&metadata) {
        bail!("Expected a regular directory at {}", path.display());
    }
    Ok(())
}

fn require_regular_file(path: &Path) -> Result<()> {
    let metadata =
        fs::symlink_metadata(path).with_context(|| format!("Missing file {}", path.display()))?;
    if !metadata.is_file() || crate::fs_utils::is_link_or_reparse(&metadata) {
        bail!("Expected a regular file at {}", path.display());
    }
    Ok(())
}

fn require_exact_names(directory: &Path, expected: &[&str]) -> Result<()> {
    require_directory(directory)?;
    let mut actual = fs::read_dir(directory)?
        .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
        .collect::<std::io::Result<Vec<_>>>()?;
    let mut expected = expected
        .iter()
        .map(|name| (*name).to_string())
        .collect::<Vec<_>>();
    actual.sort();
    expected.sort();
    if actual != expected {
        bail!("Unexpected or missing entries in {}", directory.display());
    }
    Ok(())
}

fn directory_children(directory: &Path) -> Result<Vec<PathBuf>> {
    require_directory(directory)?;
    let mut result = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        require_directory(&entry.path())?;
        result.push(entry.path());
    }
    result.sort();
    Ok(result)
}

fn regular_file_children(directory: &Path) -> Result<Vec<PathBuf>> {
    require_directory(directory)?;
    let mut result = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        require_regular_file(&entry.path())?;
        result.push(entry.path());
    }
    result.sort();
    Ok(result)
}

fn read_metadata<T: serde::de::DeserializeOwned>(directory: &Path) -> Result<T> {
    let path = repository::metadata_path(directory);
    require_regular_file(&path)?;
    repository::read_metadata_json(directory)
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    left.starts_with(right) || right.starts_with(left)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{FullGame, Game};
    use crate::persistence::MockGameRepository;
    use crate::repository::RepositoryEntityKind;
    use crate::sync::{RemoteGameMetadata, RemoteSlot, RemoteSlotMetadata};
    use chrono::Utc;
    use tempfile::tempdir;

    #[test]
    fn sqlite_failure_after_metadata_publish_is_retryable() {
        let bundle_root = tempdir().unwrap();
        repository::ensure_repository_root(bundle_root.path()).unwrap();
        fs::create_dir_all(bundle_root.path().join("retired-versions")).unwrap();

        let now = Utc::now();
        let game_uuid = Uuid::new_v4();
        let track_uuid = Uuid::new_v4();
        let slot_uuid = Uuid::new_v4();
        let game_slug =
            repository::build_folder_slug(RepositoryEntityKind::Game, "Retry Fixture", game_uuid);
        let track_slug =
            repository::build_folder_slug(RepositoryEntityKind::TrackedDir, "Saves", track_uuid);
        let slot_slug =
            repository::build_folder_slug(RepositoryEntityKind::Slot, "Default", slot_uuid);
        let game = crate::sync::RemoteGame {
            metadata: RemoteGameMetadata {
                entity: "game".into(),
                uuid: game_uuid,
                title: "Retry Fixture".into(),
                folder_slug: game_slug.clone(),
                icon: None,
                created_at: now,
                updated_at: now,
            },
            tracked_dirs: vec![TrackedDirMetadata {
                entity: "tracked_dir".into(),
                uuid: track_uuid,
                game_uuid,
                path: "C:/source/saves".into(),
                label: Some("Saves".into()),
                exclusions: Vec::new(),
                folder_slug: track_slug,
                retired_at: None,
                created_at: now,
                updated_at: now,
            }],
            slots: vec![RemoteSlot {
                metadata: RemoteSlotMetadata {
                    entity: "slot".into(),
                    uuid: slot_uuid,
                    game_uuid,
                    name: "Default".into(),
                    folder_slug: slot_slug,
                    created_at: now,
                    updated_at: now,
                },
                versions: Vec::new(),
            }],
        };
        write_game_tree(bundle_root.path(), &game).unwrap();
        repository::write_json_file(
            &bundle_root.path().join("export.json"),
            &BundleManifest {
                format_version: BUNDLE_FORMAT_VERSION,
                game_uuid,
                exported_at: now,
                slot_count: 1,
                version_count: 0,
                tracked_dir_count: 1,
                blob_count: 0,
                retired_version_count: 0,
            },
        )
        .unwrap();
        load_bundle(bundle_root.path()).unwrap();

        let destination = tempdir().unwrap();
        repository::ensure_repository_root(destination.path()).unwrap();
        let mapped_path = tempdir().unwrap();
        let mappings = HashMap::from([(track_uuid, mapped_path.path().to_path_buf())]);
        let mut failing_repo = MockGameRepository::new();
        failing_repo.expect_get_games().returning(|| Ok(Vec::new()));
        failing_repo
            .expect_import_remote_game()
            .returning(|_, _| Err(anyhow::anyhow!("injected SQLite transaction failure")));

        let error = import_game(
            destination.path(),
            &failing_repo,
            bundle_root.path(),
            &mappings,
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("injected SQLite transaction failure")
        );
        assert_eq!(
            fs::read_dir(repository::games_dir(destination.path()))
                .unwrap()
                .count(),
            1
        );
        assert!(
            crate::integrity::check_repository(destination.path(), true)
                .issues
                .is_empty()
        );

        let game_path = repository::game_dir(destination.path(), &game_slug);
        let mut succeeding_repo = MockGameRepository::new();
        succeeding_repo
            .expect_get_games()
            .returning(|| Ok(Vec::new()));
        let persisted = FullGame {
            game: Game {
                id: 1,
                uuid: Some(game_uuid.to_string()),
                title: "Retry Fixture".into(),
                title_normalized: Some("retry fixture".into()),
                folder_slug: Some(game_slug.clone()),
                active_slot_id: None,
                icon_path: None,
            },
            tracked_dirs: Vec::new(),
            slots: Vec::new(),
            active_slot: None,
            versions: Vec::new(),
            last_version: None,
        };
        succeeding_repo
            .expect_import_remote_game()
            .returning(move |_, _| Ok(persisted.clone()));
        let report = import_game(
            destination.path(),
            &succeeding_repo,
            bundle_root.path(),
            &HashMap::new(),
        )
        .unwrap();
        assert_eq!(report.game_uuid, game_uuid);
        let imported = read_game_tree(destination.path(), &game_slug, false).unwrap();
        assert_eq!(
            imported.tracked_dirs[0].path,
            mapped_path.path().to_string_lossy()
        );
        assert!(
            crate::integrity::check_repository(destination.path(), true)
                .issues
                .is_empty()
        );
        assert!(game_path.exists());
    }
}
