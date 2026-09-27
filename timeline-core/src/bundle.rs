//! History bundles: a store holding one game, plus `export.json`. Export and
//! import are transfers with the `Complete` policy; this module only adds the
//! bundle manifest, preflight checks and tracked-path mapping.

use crate::history::{History, same_path};
use crate::store::{
    GameRecord, Store, normalize_display_name, read_json, rename_no_replace, write_json,
};
use crate::transfer::{Completeness, Heal, Transfer};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use uuid::Uuid;

const BUNDLE_FORMAT: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ExportManifest {
    bundle_format: u32,
    game_uuid: Uuid,
    exported_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BundleTrackedDir {
    pub uuid: Uuid,
    pub label: Option<String>,
    pub source_path_hint: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HistoryBundleSummary {
    pub game_uuid: Uuid,
    pub title: String,
    pub slot_count: usize,
    pub version_count: usize,
    pub tracked_dir_count: usize,
    pub blob_count: usize,
    pub retired_version_count: usize,
    pub active_tracked_dirs: Vec<BundleTrackedDir>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HistoryTransferReport {
    pub game_uuid: Uuid,
    pub title: String,
    pub slot_count: usize,
    pub version_count: usize,
    pub tracked_dir_count: usize,
    pub blob_count: usize,
    pub retired_version_count: usize,
}

struct Bundle {
    store: Store,
    game: GameRecord,
    retired: HashMap<Uuid, crate::store::RetirementMarker>,
}

fn open(path: &Path) -> Result<Bundle> {
    let store = Store::open_existing(path)
        .with_context(|| format!("{} is not a history bundle", path.display()))?;
    let manifest: ExportManifest =
        read_json(&path.join("export.json")).context("The bundle has no readable export.json")?;
    if manifest.bundle_format != BUNDLE_FORMAT {
        bail!("Unsupported bundle format {}", manifest.bundle_format);
    }
    let mut games = store.catalog()?;
    if games.len() != 1 || games[0].meta.uuid != manifest.game_uuid {
        bail!("The bundle must contain exactly the game named in export.json");
    }
    let game = games.remove(0);
    if game.meta.deleted_at.is_some() {
        bail!("The bundled game is deleted");
    }
    let retired = store.markers()?;
    Ok(Bundle {
        store,
        game,
        retired,
    })
}

fn summary(bundle: &Bundle) -> HistoryBundleSummary {
    let chunks: HashSet<&str> = bundle
        .game
        .versions()
        .filter(|v| !bundle.retired.contains_key(&v.uuid))
        .flat_map(|v| v.chunk_hashes())
        .collect();
    HistoryBundleSummary {
        game_uuid: bundle.game.meta.uuid,
        title: bundle.game.meta.title.clone(),
        slot_count: bundle.game.slots.len(),
        version_count: bundle.game.versions().count(),
        tracked_dir_count: bundle.game.tracked_dirs.len(),
        blob_count: chunks.len(),
        retired_version_count: bundle
            .game
            .versions()
            .filter(|v| bundle.retired.contains_key(&v.uuid))
            .count(),
        active_tracked_dirs: bundle
            .game
            .tracked_dirs
            .iter()
            .filter(|dir| dir.retired_at.is_none())
            .map(|dir| BundleTrackedDir {
                uuid: dir.uuid,
                label: dir.label.clone(),
                source_path_hint: dir.path.clone(),
            })
            .collect(),
    }
}

fn report(summary: HistoryBundleSummary) -> HistoryTransferReport {
    HistoryTransferReport {
        game_uuid: summary.game_uuid,
        title: summary.title,
        slot_count: summary.slot_count,
        version_count: summary.version_count,
        tracked_dir_count: summary.tracked_dir_count,
        blob_count: summary.blob_count,
        retired_version_count: summary.retired_version_count,
    }
}

pub fn inspect(path: &Path) -> Result<HistoryBundleSummary> {
    Ok(summary(&open(path)?))
}

/// Writes one game into a new directory. The caller holds the repository lock.
pub fn export(
    history: &History,
    game_id: i32,
    destination: &Path,
) -> Result<HistoryTransferReport> {
    if destination.exists() {
        bail!(
            "{} already exists; choose a new folder",
            destination.display()
        );
    }
    let parent = destination
        .parent()
        .context("The destination has no parent folder")?;
    std::fs::create_dir_all(parent)?;
    let game = history.index.get_game(game_id)?;
    let mut record = history.record(&game.game)?;
    record.slots.retain(|slot| slot.meta.deleted_at.is_none());
    let markers = history.store.markers()?;

    let staging = parent.join(format!(".export-{}", crate::store::short_random()));
    let result = (|| -> Result<()> {
        let bundle = Store::open(&staging)?;
        Transfer::new(&history.store, &bundle, Completeness::Complete).copy_game(
            &record,
            &markers,
            Heal::None,
        )?;
        write_json(
            &staging.join("export.json"),
            &ExportManifest {
                bundle_format: BUNDLE_FORMAT,
                game_uuid: record.meta.uuid,
                exported_at: Utc::now(),
            },
        )?;
        rename_no_replace(&staging, destination)
            .with_context(|| format!("Failed to publish the bundle at {}", destination.display()))
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result?;
    inspect(destination).map(report)
}

/// Adds a bundle's history to the local store without restoring anything.
/// `mappings` gives a local path for each active tracked directory the local
/// game doesn't already have. The caller holds the repository lock.
pub fn import(
    history: &History,
    path: &Path,
    mappings: &HashMap<Uuid, PathBuf>,
) -> Result<HistoryTransferReport> {
    let bundle = open(path)?;
    let mut game = bundle.game.clone();
    let local_games = history.index.get_games()?;
    let existing = local_games
        .iter()
        .find(|g| g.game.uuid == game.meta.uuid.to_string());

    // Preflight everything that could fail after files are written.
    let title = normalize_display_name(&game.meta.title);
    if let Some(other) = local_games.iter().find(|g| {
        g.game.title_normalized == title && Some(g.game.id) != existing.map(|e| e.game.id)
    }) {
        bail!(
            "A different game named '{}' already exists",
            other.game.title
        );
    }
    for version in game.versions() {
        if let Some(id) = history.index.version_id(version.uuid)? {
            let owner = history.index.get_version(id)?.game.uuid;
            if owner != game.meta.uuid.to_string() {
                bail!("Version {} already belongs to another game", version.uuid);
            }
        }
    }
    let local_record = existing.map(|e| history.record(&e.game)).transpose()?;
    let taken = history.index.tracked_paths()?;
    for dir in &mut game.tracked_dirs {
        if let Some(local) = local_record
            .as_ref()
            .and_then(|record| record.tracked_dirs.iter().find(|d| d.uuid == dir.uuid))
        {
            dir.path = local.path.clone();
            continue;
        }
        if dir.retired_at.is_some() && !mappings.contains_key(&dir.uuid) {
            continue;
        }
        let target = mappings
            .get(&dir.uuid)
            .with_context(|| format!("Choose a local folder for tracked directory {}", dir.path))?;
        if !target.is_absolute() {
            bail!("{} is not an absolute path", target.display());
        }
        let target_text = target.to_string_lossy();
        if taken.iter().any(|(id, path)| {
            Some(*id) != existing.map(|e| e.game.id) && same_path(path, &target_text)
        }) {
            bail!("{} is already tracked by another game", target.display());
        }
        dir.path = crate::path_placeholders::substitute_path_placeholders(target);
    }

    Transfer::new(&bundle.store, &history.store, Completeness::Complete).copy_game(
        &game,
        &bundle.retired,
        Heal::None,
    )?;
    history.refresh_game(game.meta.uuid)?;
    Ok(report(summary(&bundle)))
}
