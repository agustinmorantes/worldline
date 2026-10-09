//! Copies games between any two stores: local ↔ remote for sync, local →
//! bundle for export, bundle → local for import. One implementation of
//! parent ordering, chunk closure, verification and publication.

use crate::chunks;
use crate::icon;
use crate::store::{GameMeta, GameRecord, RetirementMarker, Store, VersionMeta, format_hash};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Completeness {
    /// Sync: a version whose parent or chunks haven't arrived is skipped.
    Partial,
    /// Bundles: anything missing or corrupt is an error.
    Complete,
}

/// Which versions already in the destination get their chunks re-checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Heal<'a> {
    None,
    /// Versions created since a moment, or in `pulled` (copied here recently,
    /// however old they are).
    Recent {
        since: DateTime<Utc>,
        pulled: &'a HashSet<Uuid>,
    },
    All,
}

#[derive(Debug, Default, Clone, Serialize, PartialEq, Eq)]
pub struct TransferReport {
    /// Versions newly published to the destination.
    pub copied_versions: Vec<Uuid>,
    pub chunks_copied: usize,
    pub bytes_copied: u64,
    /// Versions skipped because their parent or chunks haven't arrived yet.
    pub not_arrived: Vec<Uuid>,
}

pub struct Transfer<'a> {
    src: &'a Store,
    dst: &'a Store,
    completeness: Completeness,
    /// Whether versions' screenshots travel too (they always do in bundles).
    screenshots: bool,
    /// Chunks already checked in this run, so each is handled once.
    seen: HashSet<String>,
    pub report: TransferReport,
}

impl<'a> Transfer<'a> {
    pub fn new(src: &'a Store, dst: &'a Store, completeness: Completeness) -> Self {
        Self {
            src,
            dst,
            completeness,
            screenshots: true,
            seen: HashSet::new(),
            report: TransferReport::default(),
        }
    }

    /// Copies screenshots or leaves them behind (sync with "sync
    /// screenshots" off).
    pub fn screenshots(mut self, copy: bool) -> Self {
        self.screenshots = copy;
        self
    }

    /// Copies one game (as read from `src`) and returns the version UUIDs
    /// present in `dst` afterwards.
    pub fn copy_game(
        &mut self,
        game: &GameRecord,
        markers: &HashMap<Uuid, RetirementMarker>,
        heal: Heal<'_>,
    ) -> Result<HashSet<Uuid>> {
        let slug = &game.meta.folder_slug;
        if self.completeness == Completeness::Complete {
            // Everything that can fail is checked before any metadata is
            // written, so a failed import never leaves a partial game behind.
            ensure_acyclic(game)?;
            self.dst.check_conflicts(game)?;
            let known: HashSet<Uuid> = game.versions().map(|v| v.uuid).collect();
            for version in game.versions() {
                if let Some(parent) = version.parent_uuid.filter(|p| !known.contains(p)) {
                    bail!("version {} has a missing parent {parent}", version.uuid);
                }
                if !markers.contains_key(&version.uuid) {
                    self.copy_chunks(version)
                        .with_context(|| format!("version {} is incomplete", version.uuid))?;
                }
            }
            if let Some(wanted) = &game.meta.icon_sha256
                && self.src_icon(&game.meta)?.is_none()
            {
                bail!("the icon {wanted} of '{}' is missing", game.meta.title);
            }
        }
        // The icon travels before the metadata naming it, when that metadata
        // will win; afterwards whatever metadata won gets its icon if `src`
        // has it (an icon that hasn't arrived yet is healed on a later run).
        let wins = self
            .dst
            .game_meta(slug)?
            .is_none_or(|existing| game.meta.updated_at > existing.updated_at);
        if wins {
            self.copy_icon(&game.meta)?;
        }
        self.dst.write_game(&game.meta)?;
        if let Some(stored) = self.dst.game_meta(slug)? {
            self.copy_icon(&stored)?;
        }
        for dir in &game.tracked_dirs {
            self.dst.write_tracked_dir(slug, dir)?;
        }
        for slot in &game.slots {
            self.dst.write_slot(slug, &slot.meta)?;
        }

        let existing = self.dst.game_by_slug(slug)?;
        let mut present: HashSet<Uuid> = existing
            .iter()
            .flat_map(|record| record.versions().map(|version| version.uuid))
            .collect();
        let slot_slugs: HashMap<Uuid, &str> = game
            .slots
            .iter()
            .map(|slot| (slot.meta.uuid, slot.meta.folder_slug.as_str()))
            .collect();

        // Parent order: a version is visited once its parent is in `dst`.
        let mut pending: Vec<&VersionMeta> = game.versions().collect();
        loop {
            let before = pending.len();
            let mut deferred = Vec::new();
            for version in pending {
                if version
                    .parent_uuid
                    .is_some_and(|parent| !present.contains(&parent))
                {
                    deferred.push(version);
                    continue;
                }
                let already = present.contains(&version.uuid);
                let retired = markers.contains_key(&version.uuid);
                let check_chunks = !retired
                    && match (already, heal) {
                        (false, _) | (true, Heal::All) => true,
                        (true, Heal::Recent { since, pulled }) => {
                            version.created_at >= since || pulled.contains(&version.uuid)
                        }
                        (true, Heal::None) => false,
                    };
                if check_chunks && let Err(error) = self.copy_chunks(version) {
                    if self.completeness == Completeness::Complete {
                        return Err(
                            error.context(format!("version {} is incomplete", version.uuid))
                        );
                    }
                    if !already {
                        self.report.not_arrived.push(version.uuid);
                        continue;
                    }
                }
                self.dst
                    .publish_version(slug, slot_slugs[&version.slot_uuid], version)?;
                if !retired {
                    self.copy_screenshot(slug, slot_slugs[&version.slot_uuid], version)?;
                }
                if !already {
                    self.report.copied_versions.push(version.uuid);
                    present.insert(version.uuid);
                }
            }
            if deferred.is_empty() {
                break;
            }
            if deferred.len() == before {
                if self.completeness == Completeness::Complete {
                    bail!("version {} has a missing parent", deferred[0].uuid);
                }
                self.report
                    .not_arrived
                    .extend(deferred.iter().map(|v| v.uuid));
                break;
            }
            pending = deferred;
        }

        for marker in markers.values() {
            if marker.game_uuid == game.meta.uuid && present.contains(&marker.version_uuid) {
                self.dst.write_marker(marker)?;
            }
        }
        Ok(present)
    }

    /// `src`'s icon of the game when it is the one `meta` names.
    fn src_icon(&self, meta: &GameMeta) -> Result<Option<Vec<u8>>> {
        let Some(wanted) = &meta.icon_sha256 else {
            return Ok(None);
        };
        Ok(self
            .src
            .read_icon(&meta.folder_slug)?
            .filter(|png| icon::sha256(png) == *wanted))
    }

    /// Gives `dst` the icon `meta` names, when `src` has it. Tombstoned games
    /// need none.
    fn copy_icon(&self, meta: &GameMeta) -> Result<()> {
        if meta.deleted_at.is_some() {
            return Ok(());
        }
        let Some(wanted) = &meta.icon_sha256 else {
            return Ok(());
        };
        let present = self.dst.read_icon(&meta.folder_slug)?;
        if present.is_some_and(|png| icon::sha256(&png) == *wanted) {
            return Ok(());
        }
        match self.src_icon(meta)? {
            Some(png) => self.dst.write_icon(&meta.folder_slug, &png),
            None => Ok(()),
        }
    }

    /// Gives `dst` the version's screenshot when only `src` has it. One taken
    /// after the version was copied arrives on a later run.
    fn copy_screenshot(
        &self,
        game_slug: &str,
        slot_slug: &str,
        version: &VersionMeta,
    ) -> Result<()> {
        let path =
            |store: &Store| store.screenshot_path(game_slug, slot_slug, &version.folder_slug);
        if !self.screenshots || path(self.dst).exists() {
            return Ok(());
        }
        let source = path(self.src);
        match std::fs::read(&source) {
            Ok(jpeg) => {
                self.dst
                    .write_screenshot(game_slug, slot_slug, &version.folder_slug, &jpeg)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => {
                Err(error).with_context(|| format!("failed to read {}", source.display()))
            }
        }
    }

    fn copy_chunks(&mut self, version: &VersionMeta) -> Result<()> {
        for hash in version.chunk_hashes() {
            if self.seen.contains(hash) {
                continue;
            }
            if !self.dst.has_chunk(hash) {
                let compressed = chunks::read_compressed(self.src, hash)?;
                let valid = zstd::stream::decode_all(compressed.as_slice())
                    .is_ok_and(|raw| format_hash(&Sha256::digest(&raw)) == hash);
                if !valid {
                    if self.completeness == Completeness::Partial {
                        // Move the damaged copy out of the way so the device
                        // holding the real bytes re-uploads it (writer healing).
                        let _ = self.src.quarantine_chunk(hash);
                    }
                    bail!("chunk {hash} failed verification");
                }
                chunks::publish_chunk(self.dst, hash, &compressed)
                    .with_context(|| format!("failed to copy chunk {hash}"))?;
                self.report.chunks_copied += 1;
                self.report.bytes_copied += compressed.len() as u64;
            }
            self.seen.insert(hash.to_string());
        }
        Ok(())
    }
}

/// Rejects a version whose parent chain loops instead of reaching a root.
pub(crate) fn ensure_acyclic(game: &GameRecord) -> Result<()> {
    let parents: HashMap<Uuid, Option<Uuid>> = game
        .versions()
        .map(|version| (version.uuid, version.parent_uuid))
        .collect();
    for version in game.versions() {
        let mut seen = HashSet::from([version.uuid]);
        let mut current = version.parent_uuid;
        while let Some(parent) = current {
            if !seen.insert(parent) {
                bail!("version {} has cyclic ancestry", version.uuid);
            }
            current = parents.get(&parent).copied().flatten();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunks::CaptureRoot;
    use crate::store::{GameMeta, SlotMeta, SlotRecord, VersionReason, folder_slug};
    use tempfile::tempdir;

    fn version(
        game: &GameMeta,
        slot: &SlotMeta,
        parent: Option<Uuid>,
        files: Vec<crate::store::FileEntry>,
    ) -> VersionMeta {
        let uuid = Uuid::new_v4();
        let now = Utc::now();
        VersionMeta {
            uuid,
            game_uuid: game.uuid,
            slot_uuid: slot.uuid,
            parent_uuid: parent,
            version_number: 1,
            folder_slug: folder_slug('v', "v", uuid),
            reason: VersionReason::Manual,
            tag: None,
            created_at: now,
            updated_at: now,
            origin_device: None,
            files,
        }
    }

    fn fixture(src: &Store) -> (GameRecord, VersionMeta, VersionMeta) {
        let now = Utc::now();
        let uuid = Uuid::new_v4();
        let game = GameMeta {
            uuid,
            title: "Game".into(),
            folder_slug: folder_slug('g', "Game", uuid),
            created_at: now,
            updated_at: now,
            deleted_at: None,
            icon_sha256: None,
            accent_hue: None,
            accent_hue_source: Default::default(),
        };
        let slot_uuid = Uuid::new_v4();
        let slot = SlotMeta {
            uuid: slot_uuid,
            game_uuid: uuid,
            folder_slug: folder_slug('s', "Main", slot_uuid),
            name: "Main".into(),
            created_at: now,
            updated_at: now,
            deleted_at: None,
            seen_versions: Vec::new(),
        };
        let live = tempdir().unwrap();
        std::fs::write(live.path().join("save.dat"), b"progress").unwrap();
        let files = chunks::capture(
            src,
            &[CaptureRoot {
                uuid: Uuid::from_u128(7),
                path: live.path(),
                exclusions: &[],
            }],
        )
        .unwrap();
        let parent = version(&game, &slot, None, files.clone());
        let child = version(&game, &slot, Some(parent.uuid), files);
        // Child listed first: the transfer must still order by parent.
        let record = GameRecord {
            meta: game,
            tracked_dirs: vec![],
            slots: vec![SlotRecord {
                meta: slot,
                versions: vec![child.clone(), parent.clone()],
            }],
        };
        (record, parent, child)
    }

    #[test]
    fn child_before_parent_is_ordered_and_orphans_wait_for_their_parent() {
        let (a, b) = (tempdir().unwrap(), tempdir().unwrap());
        let (src, dst) = (
            Store::open(a.path()).unwrap(),
            Store::open(b.path()).unwrap(),
        );
        let (mut record, parent, child) = fixture(&src);

        // Only the child has arrived.
        record.slots[0].versions.retain(|v| v.uuid == child.uuid);
        let mut transfer = Transfer::new(&src, &dst, Completeness::Partial);
        let present = transfer
            .copy_game(&record, &HashMap::new(), Heal::None)
            .unwrap();
        assert!(present.is_empty());
        assert_eq!(transfer.report.not_arrived, vec![child.uuid]);

        record.slots[0].versions = vec![child.clone(), parent.clone()];
        let mut transfer = Transfer::new(&src, &dst, Completeness::Partial);
        let present = transfer
            .copy_game(&record, &HashMap::new(), Heal::None)
            .unwrap();
        assert_eq!(present.len(), 2);
        assert_eq!(
            transfer.report.chunks_copied, 1,
            "a shared chunk is copied once"
        );
        assert!(
            dst.game(record.meta.uuid)
                .unwrap()
                .unwrap()
                .version(child.uuid)
                .is_some()
        );
    }

    #[test]
    fn missing_chunks_are_not_arrived_for_sync_and_errors_for_bundles() {
        let (a, b) = (tempdir().unwrap(), tempdir().unwrap());
        let (src, dst) = (
            Store::open(a.path()).unwrap(),
            Store::open(b.path()).unwrap(),
        );
        let (record, parent, _) = fixture(&src);
        std::fs::remove_file(src.chunk_path(&parent.files[0].chunks[0]).unwrap()).unwrap();

        let mut transfer = Transfer::new(&src, &dst, Completeness::Partial);
        assert!(
            transfer
                .copy_game(&record, &HashMap::new(), Heal::None)
                .unwrap()
                .is_empty()
        );
        assert_eq!(transfer.report.not_arrived.len(), 2);

        let mut transfer = Transfer::new(&src, &dst, Completeness::Complete);
        assert!(
            transfer
                .copy_game(&record, &HashMap::new(), Heal::None)
                .is_err()
        );
    }

    #[test]
    fn healing_reuploads_chunks_deleted_from_the_destination() {
        let (a, b) = (tempdir().unwrap(), tempdir().unwrap());
        let (src, dst) = (
            Store::open(a.path()).unwrap(),
            Store::open(b.path()).unwrap(),
        );
        let (record, parent, _) = fixture(&src);
        Transfer::new(&src, &dst, Completeness::Partial)
            .copy_game(&record, &HashMap::new(), Heal::None)
            .unwrap();
        let chunk = dst.chunk_path(&parent.files[0].chunks[0]).unwrap();
        std::fs::remove_file(&chunk).unwrap();

        Transfer::new(&src, &dst, Completeness::Partial)
            .copy_game(&record, &HashMap::new(), Heal::None)
            .unwrap();
        assert!(
            !chunk.exists(),
            "without healing, present versions are trusted"
        );
        Transfer::new(&src, &dst, Completeness::Partial)
            .copy_game(
                &record,
                &HashMap::new(),
                Heal::Recent {
                    since: Utc::now() - chrono::Duration::hours(1),
                    pulled: &HashSet::new(),
                },
            )
            .unwrap();
        assert!(chunk.exists());
    }

    #[test]
    fn a_cyclic_bundle_is_rejected_before_anything_is_written() {
        let (a, b) = (tempdir().unwrap(), tempdir().unwrap());
        let (src, dst) = (
            Store::open(a.path()).unwrap(),
            Store::open(b.path()).unwrap(),
        );
        let (mut record, parent, child) = fixture(&src);
        for version in &mut record.slots[0].versions {
            if version.uuid == parent.uuid {
                version.parent_uuid = Some(child.uuid);
            }
        }
        assert!(
            Transfer::new(&src, &dst, Completeness::Complete)
                .copy_game(&record, &HashMap::new(), Heal::None)
                .is_err()
        );
        assert!(dst.find_game(record.meta.uuid).unwrap().is_none());
        assert!(dst.catalog().unwrap().is_empty());
    }

    #[test]
    fn healing_covers_old_versions_that_were_pulled_recently() {
        let (a, b) = (tempdir().unwrap(), tempdir().unwrap());
        let (src, dst) = (
            Store::open(a.path()).unwrap(),
            Store::open(b.path()).unwrap(),
        );
        let (mut record, parent, child) = fixture(&src);
        for version in &mut record.slots[0].versions {
            version.created_at -= chrono::Duration::days(30);
        }
        Transfer::new(&src, &dst, Completeness::Partial)
            .copy_game(&record, &HashMap::new(), Heal::None)
            .unwrap();
        let chunk = dst.chunk_path(&parent.files[0].chunks[0]).unwrap();
        std::fs::remove_file(&chunk).unwrap();
        let since = Utc::now() - chrono::Duration::hours(1);

        Transfer::new(&src, &dst, Completeness::Partial)
            .copy_game(
                &record,
                &HashMap::new(),
                Heal::Recent {
                    since,
                    pulled: &HashSet::new(),
                },
            )
            .unwrap();
        assert!(!chunk.exists(), "old and not pulled: not healed");
        Transfer::new(&src, &dst, Completeness::Partial)
            .copy_game(
                &record,
                &HashMap::new(),
                Heal::Recent {
                    since,
                    pulled: &HashSet::from([child.uuid]),
                },
            )
            .unwrap();
        assert!(chunk.exists());
    }
}
