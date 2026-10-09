//! History bundles: a zip of a store holding one game, plus `export.json`.
//! Export and import are transfers with the `Complete` policy; this module only
//! adds the zip packaging, bundle manifest, preflight checks and tracked-path
//! mapping.

use crate::history::{History, overlapping_paths, same_path};
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct BundleTrackedDir {
    pub uuid: Uuid,
    pub label: Option<String>,
    pub source_path_hint: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
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
    // Dropped last, after `store` lets go of the files.
    _unpacked: Option<Unpacked>,
}

/// A temporary directory holding an unzipped bundle, removed on drop.
struct Unpacked(PathBuf);

impl Drop for Unpacked {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Zips the contents of `dir` into the new file `file`.
fn pack(dir: &Path, file: &Path) -> Result<()> {
    let mut zip = zip::ZipWriter::new(
        std::fs::File::create_new(file)
            .with_context(|| format!("Failed to create {}", file.display()))?,
    );
    let options = zip::write::SimpleFileOptions::default();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        for entry in std::fs::read_dir(&current)? {
            let path = entry?.path();
            let name = path
                .strip_prefix(dir)?
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            if path.is_dir() {
                zip.add_directory(name, options)?;
                pending.push(path);
            } else {
                zip.start_file(name, options)?;
                std::io::copy(&mut std::fs::File::open(&path)?, &mut zip)?;
            }
        }
    }
    zip.finish()?.sync_all()?;
    Ok(())
}

fn unpack(path: &Path) -> Result<Unpacked> {
    let file =
        std::fs::File::open(path).with_context(|| format!("Failed to open {}", path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("{} is not a history bundle", path.display()))?;
    let unpacked = Unpacked(
        std::env::temp_dir().join(format!("worldline-bundle-{}", crate::store::short_random())),
    );
    archive
        .extract(&unpacked.0)
        .with_context(|| format!("Failed to unzip {}", path.display()))?;
    Ok(unpacked)
}

fn open(path: &Path) -> Result<Bundle> {
    let unpacked = unpack(path)?;
    let mut bundle = load(&unpacked.0, path)?;
    bundle._unpacked = Some(unpacked);
    Ok(bundle)
}

/// Opens the unzipped bundle at `root`; `path` names it in errors.
fn load(root: &Path, path: &Path) -> Result<Bundle> {
    let store = Store::open_existing(root)
        .with_context(|| format!("{} is not a history bundle", path.display()))?;
    let manifest: ExportManifest =
        read_json(&root.join("export.json")).context("The bundle has no readable export.json")?;
    if manifest.bundle_format != BUNDLE_FORMAT {
        bail!("Unsupported bundle format {}", manifest.bundle_format);
    }
    // The catalog skips what it can't load; a bundle must not silently lose it.
    if let Some(path) = store.first_unloadable_metadata()? {
        bail!("The bundle is damaged: {} is unreadable", path.display());
    }
    let mut games = store.catalog()?;
    if games.len() != 1 || games[0].meta.uuid != manifest.game_uuid {
        bail!("The bundle must contain exactly the game named in export.json");
    }
    let game = games.remove(0);
    crate::transfer::ensure_acyclic(&game)?;
    if game.meta.deleted_at.is_some() {
        bail!("The bundled game is deleted");
    }
    let retired = store.markers()?;
    Ok(Bundle {
        store,
        game,
        retired,
        _unpacked: None,
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

/// Writes one game into a new zip file. The caller holds the repository lock.
pub fn export(
    history: &History,
    game_id: i32,
    destination: &Path,
) -> Result<HistoryTransferReport> {
    if destination.exists() {
        bail!(
            "{} already exists; choose a new file name",
            destination.display()
        );
    }
    let parent = destination
        .parent()
        .context("The destination has no parent folder")?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("Failed to create {}", parent.display()))?;
    let game = history.index.get_game(game_id)?;
    let mut record = history.record(&game.game)?;
    record.slots.retain(|slot| slot.meta.deleted_at.is_none());
    let markers = history.store.markers()?;

    let staging = parent.join(format!(".export-{}", crate::store::short_random()));
    let staging_zip = staging.with_extension("zip");
    let result = (|| -> Result<HistoryBundleSummary> {
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
        let summary = summary(&load(&staging, destination)?);
        pack(&staging, &staging_zip)?;
        rename_no_replace(&staging_zip, destination).with_context(|| {
            format!("Failed to publish the bundle at {}", destination.display())
        })?;
        Ok(summary)
    })();
    let _ = std::fs::remove_dir_all(&staging);
    if result.is_err() {
        let _ = std::fs::remove_file(&staging_zip);
    }
    result.map(report)
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
    if history
        .store
        .find_game(game.meta.uuid)?
        .is_some_and(|meta| meta.deleted_at.is_some())
    {
        bail!(
            "'{}' was deleted on this device and can't be imported again",
            game.meta.title
        );
    }
    for slot in &game.slots {
        if let Some(id) = history.index.slot_id(slot.meta.uuid)? {
            let owner = history.index.get_slot(id)?.slot.game_id;
            if Some(owner) != existing.map(|e| e.game.id) {
                bail!("Slot {} already belongs to another game", slot.meta.uuid);
            }
        }
    }
    let targets: Vec<&PathBuf> = mappings.values().collect();
    if targets.iter().enumerate().any(|(i, a)| {
        targets[i + 1..]
            .iter()
            .any(|b| same_path(&a.to_string_lossy(), &b.to_string_lossy()))
    }) {
        bail!("Two tracked directories can't share one folder");
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
        if let Some((_, path)) = taken.iter().find(|(id, path)| {
            Some(*id) != existing.map(|e| e.game.id) && overlapping_paths(path, &target_text)
        }) {
            bail!(
                "{} overlaps {path}, which another game tracks",
                target.display()
            );
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
