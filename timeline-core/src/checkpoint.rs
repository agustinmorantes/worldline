use crate::content_store::capture_snapshot;
use crate::models::{FullGame, FullVersion};
use crate::persistence::VersionRepository;
use crate::repository::{self, VersionMetadata, VersionReason};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub enum CheckpointOutcome {
    Unchanged(FullVersion),
    Created(FullVersion),
}

pub fn checkpoint_current_state(
    working_base: &Path,
    versions_repo: &dyn VersionRepository,
    game: &FullGame,
    reason: VersionReason,
) -> Result<CheckpointOutcome> {
    repository::with_exclusive_lock(working_base, || {
        let snapshot = capture_snapshot(working_base, &game.tracked_dirs)
            .context("failed to capture tracked directories")?;

        if let Some(active_version) = game.last_version.as_ref()
            && let Some(metadata) = active_version_metadata(working_base, game, active_version)?
            && metadata.content_hash == snapshot.content_hash
        {
            let version = versions_repo
                .get_version(active_version.id)
                .context("failed to load unchanged active version")?;
            return Ok(CheckpointOutcome::Unchanged(version));
        }

        let version = versions_repo
            .create_version_from_snapshot(game.game.id, reason, snapshot)
            .context("failed to create checkpoint version")?;
        Ok(CheckpointOutcome::Created(version))
    })
}

fn active_version_metadata(
    working_base: &Path,
    game: &FullGame,
    version: &crate::models::Version,
) -> Result<Option<VersionMetadata>> {
    let Ok(path) = version_metadata_path(working_base, game, version) else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }

    let entity_dir = path
        .parent()
        .context("version metadata path has no parent")?;
    repository::read_metadata_json(entity_dir).map(Some)
}

fn version_metadata_path(
    working_base: &Path,
    game: &FullGame,
    version: &crate::models::Version,
) -> Result<PathBuf> {
    let game_folder_slug = game
        .game
        .folder_slug
        .as_deref()
        .context("game is missing a folder slug")?;
    let version_folder_slug = version
        .folder_slug
        .as_deref()
        .context("version is missing a folder slug")?;
    let active_slot = game
        .active_slot
        .as_ref()
        .context("game is missing an active slot")?;

    let game_dir = repository::game_dir(working_base, game_folder_slug);
    let slot_dir = repository::slot_dir(&game_dir, &active_slot.folder_slug);
    let version_dir = repository::version_dir(&slot_dir, version_folder_slug);
    Ok(repository::metadata_path(&version_dir))
}
