use diesel::SqliteConnection;
use diesel::r2d2::{ConnectionManager, Pool};
use diesel_migrations::MigrationHarness;
use savegame_timeline_core::checkpoint::{self, CheckpointOutcome};
use savegame_timeline_core::models::{FullGame, GameForm};
use savegame_timeline_core::persistence::sqlite::{
    SqliteGameRepository, SqliteScanRootsRepository, SqliteVersionRepository,
};
use savegame_timeline_core::pruning::{self, RetirementMarker};
use savegame_timeline_core::repository::{self, TrackedDirMetadata, VersionReason};
use savegame_timeline_core::sync::{self, Divergence, SyncSettings};
use savegame_timeline_core::{MIGRATIONS, TimelineCore};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tempfile::{TempDir, tempdir};
use uuid::Uuid;

struct DeviceHarness {
    core: TimelineCore,
    versions: Arc<SqliteVersionRepository>,
    data: TempDir,
    live: TempDir,
    _notifications: savegame_timeline_core::detection::NotificationReceiver,
}

impl DeviceHarness {
    fn new(remote: &Path) -> Self {
        let data = tempdir().unwrap();
        let live = tempdir().unwrap();
        fs::create_dir_all(data.path().join("cache")).unwrap();
        fs::write(data.path().join("cache").join("manifest.yaml"), "{}").unwrap();
        let repository_root = data.path().join("repository");
        let manager = ConnectionManager::<SqliteConnection>::new(":memory:");
        let pool = Pool::builder().max_size(1).build(manager).unwrap();
        pool.get()
            .unwrap()
            .run_pending_migrations(MIGRATIONS)
            .unwrap();
        let games = Arc::new(SqliteGameRepository::with_repository_root(
            pool.clone(),
            repository_root.clone(),
        ));
        let versions = Arc::new(SqliteVersionRepository::with_repository_root(
            pool.clone(),
            repository_root.clone(),
        ));
        let roots = Arc::new(SqliteScanRootsRepository::new(pool));
        let (core, notifications) = TimelineCore::new(
            games,
            versions.clone(),
            roots,
            repository_root,
            data.path().join("icons"),
            Some(5.0),
        )
        .unwrap();
        core.update_sync_settings(SyncSettings {
            remote_path: Some(remote.to_string_lossy().into_owned()),
        })
        .unwrap();
        Self {
            core,
            versions,
            data,
            live,
            _notifications: notifications,
        }
    }

    fn add_game(&self, title: &str, bytes: &[u8]) -> FullGame {
        fs::write(self.live.path().join("save.dat"), bytes).unwrap();
        let game = self
            .core
            .save_game(
                GameForm {
                    id: None,
                    uuid: None,
                    title,
                    title_normalized: None,
                    folder_slug: None,
                    active_slot_id: None,
                    icon_path: None,
                },
                Some(vec![self.live.path().to_str().unwrap()]),
            )
            .unwrap();
        self.checkpoint(game.id);
        self.core.get_game(game.id).unwrap()
    }

    fn checkpoint(&self, game_id: i32) -> FullGame {
        let game = self.core.get_game(game_id).unwrap();
        let outcome = checkpoint::checkpoint_current_state(
            &self.data.path().join("repository"),
            self.versions.as_ref(),
            &game,
            VersionReason::Manual,
        )
        .unwrap();
        assert!(matches!(outcome, CheckpointOutcome::Created(_)));
        self.core.get_game(game_id).unwrap()
    }

    fn write(&self, bytes: &[u8]) {
        fs::write(self.live.path().join("save.dat"), bytes).unwrap();
    }

    fn bytes(&self) -> Vec<u8> {
        fs::read(self.live.path().join("save.dat")).unwrap()
    }
}

impl Drop for DeviceHarness {
    fn drop(&mut self) {
        self.core.shutdown();
    }
}

#[test]
fn retired_version_keeps_ancestry_without_blob_or_restore() {
    let remote = tempdir().unwrap();
    let first = DeviceHarness::new(remote.path());
    let game = first.add_game("Retirement Game", b"first");
    first.write(b"middle");
    let game = first.checkpoint(game.game.id);
    let middle = game.last_version.as_ref().unwrap().clone();
    first.write(b"latest");
    let game = first.checkpoint(game.game.id);
    first.core.push_sync().unwrap();
    assert_eq!(
        first.core.preview_pruning().unwrap().retire_versions.len(),
        0
    );

    let local_root = first.data.path().join("repository");
    let middle_meta: repository::VersionMetadata =
        repository::read_metadata_json(&repository::version_dir(
            &repository::slot_dir(
                &repository::game_dir(&local_root, game.game.folder_slug.as_deref().unwrap()),
                &game.active_slot.as_ref().unwrap().folder_slug,
            ),
            middle.folder_slug.as_deref().unwrap(),
        ))
        .unwrap();
    let middle_uuid = middle_meta.uuid;
    pruning::write_marker(
        &local_root,
        &RetirementMarker {
            version_uuid: middle_uuid,
            slot_uuid: middle_meta.slot_uuid.unwrap(),
            content_hash: middle_meta.content_hash.clone(),
            retired_at: chrono::Utc::now(),
        },
    )
    .unwrap();
    first.core.push_sync().unwrap();
    let report = first.core.run_pruning().unwrap();
    assert_eq!(report.local_gc.reclaimable_blobs, 1);
    assert_eq!(report.remote_gc.unwrap().reclaimable_blobs, 1);
    first.core.push_sync().unwrap();
    assert!(
        !savegame_timeline_core::content_store::blob_path(
            remote.path(),
            &middle_meta.files[0].sha256
        )
        .unwrap()
        .exists()
    );

    let second = DeviceHarness::new(remote.path());
    let game_uuid = Uuid::parse_str(game.game.uuid.as_deref().unwrap()).unwrap();
    point_remote_tracked_dir_at(remote.path(), game_uuid, second.live.path());
    let adopted = second.core.adopt_remote_game(game_uuid).unwrap();
    assert_eq!(adopted.versions.len(), 3);
    let middle_id = adopted
        .versions
        .iter()
        .find(|v| v.uuid.as_deref() == Some(&middle_uuid.to_string()))
        .unwrap()
        .id;
    let first_id = adopted
        .versions
        .iter()
        .find(|v| v.version_number == 1)
        .unwrap()
        .id;
    let latest = adopted
        .versions
        .iter()
        .find(|v| v.version_number == 3)
        .unwrap();
    assert_eq!(
        adopted
            .versions
            .iter()
            .find(|v| v.id == middle_id)
            .unwrap()
            .parent_id,
        Some(first_id)
    );
    assert_eq!(latest.parent_id, Some(middle_id));
    assert!(second.core.restore_version(middle_id).is_err());
    assert_eq!(second.bytes(), b"latest");
    let catalog = sync::load_catalog(remote.path()).unwrap();
    assert_eq!(catalog.games[0].slots[0].versions.len(), 3);
    assert_eq!(catalog.retired_versions.len(), 1);
}

#[test]
fn local_pruning_reclaims_only_eligible_payload() {
    let remote = tempdir().unwrap();
    let device = DeviceHarness::new(remote.path());
    device
        .core
        .update_sync_settings(SyncSettings { remote_path: None })
        .unwrap();
    let mut game = device.add_game("Local Pruning Game", b"first");
    for bytes in [b"second".as_slice(), b"third", b"fourth"] {
        device.write(bytes);
        game = device.checkpoint(game.game.id);
    }
    let root = device.data.path().join("repository");
    let game_dir = repository::game_dir(&root, game.game.folder_slug.as_deref().unwrap());
    let slot_dir = repository::slot_dir(&game_dir, &game.active_slot.as_ref().unwrap().folder_slug);
    for version in &game.versions {
        let version_dir =
            repository::version_dir(&slot_dir, version.folder_slug.as_deref().unwrap());
        let mut metadata: repository::VersionMetadata =
            repository::read_metadata_json(&version_dir).unwrap();
        metadata.created_at = chrono::DateTime::parse_from_rfc3339("2020-01-06T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc)
            + chrono::Duration::minutes(i64::from(version.version_number));
        repository::write_metadata_json(&version_dir, &metadata).unwrap();
    }
    let second = game
        .versions
        .iter()
        .find(|v| v.version_number == 2)
        .unwrap();
    let first = game
        .versions
        .iter()
        .find(|v| v.version_number == 1)
        .unwrap();
    let expected = Uuid::parse_str(second.uuid.as_deref().unwrap()).unwrap();
    assert_eq!(
        device.core.preview_pruning().unwrap().retire_versions,
        vec![expected]
    );
    let report = device.core.run_pruning().unwrap();
    assert_eq!(report.retired_versions, vec![expected]);
    assert_eq!(report.local_gc.reclaimable_blobs, 1);
    assert_eq!(report.local_gc.reclaimable_bytes, b"second".len() as u64);
    assert!(device.core.restore_version(second.id).is_err());
    device.core.restore_version(first.id).unwrap();
    assert_eq!(device.bytes(), b"first");
    assert!(
        device
            .core
            .preview_pruning()
            .unwrap()
            .retire_versions
            .is_empty()
    );
}

#[test]
fn representative_history_reclaims_seventy_five_percent_of_payload_bytes() {
    let remote = tempdir().unwrap();
    let device = DeviceHarness::new(remote.path());
    device
        .core
        .update_sync_settings(SyncSettings { remote_path: None })
        .unwrap();
    let mut game = device.add_game("Long History", b"save-00");
    for index in 1..12 {
        device.write(format!("save-{index:02}").as_bytes());
        game = device.checkpoint(game.game.id);
    }
    let root = device.data.path().join("repository");
    let game_dir = repository::game_dir(&root, game.game.folder_slug.as_deref().unwrap());
    let slot_dir = repository::slot_dir(&game_dir, &game.active_slot.as_ref().unwrap().folder_slug);
    for version in &game.versions {
        let path = repository::version_dir(&slot_dir, version.folder_slug.as_deref().unwrap());
        let mut metadata: repository::VersionMetadata =
            repository::read_metadata_json(&path).unwrap();
        metadata.created_at = chrono::DateTime::parse_from_rfc3339("2020-01-06T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc)
            + chrono::Duration::minutes(i64::from(version.version_number));
        repository::write_metadata_json(&path, &metadata).unwrap();
    }
    let before = 12 * b"save-00".len();
    let report = device.core.run_pruning().unwrap();
    assert_eq!(report.retired_versions.len(), 9);
    assert_eq!(report.local_gc.reclaimable_bytes as usize, before * 3 / 4);
    let first = game
        .versions
        .iter()
        .find(|v| v.version_number == 1)
        .unwrap();
    device.core.restore_version(first.id).unwrap();
    assert_eq!(device.bytes(), b"save-00");
    device
        .core
        .restore_version(game.last_version.unwrap().id)
        .unwrap();
    assert_eq!(device.bytes(), b"save-11");
}

#[test]
fn cleanup_publishes_local_history_before_remote_retirement() {
    let remote = tempdir().unwrap();
    let device = DeviceHarness::new(remote.path());
    let mut game = device.add_game("Unpublished History", b"first");
    for bytes in [b"second".as_slice(), b"third", b"fourth"] {
        device.write(bytes);
        game = device.checkpoint(game.game.id);
    }
    let root = device.data.path().join("repository");
    let game_dir = repository::game_dir(&root, game.game.folder_slug.as_deref().unwrap());
    let slot_dir = repository::slot_dir(&game_dir, &game.active_slot.as_ref().unwrap().folder_slug);
    for version in &game.versions {
        let path = repository::version_dir(&slot_dir, version.folder_slug.as_deref().unwrap());
        let mut metadata: repository::VersionMetadata =
            repository::read_metadata_json(&path).unwrap();
        metadata.created_at = chrono::DateTime::parse_from_rfc3339("2020-01-06T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc)
            + chrono::Duration::minutes(i64::from(version.version_number));
        repository::write_metadata_json(&path, &metadata).unwrap();
    }
    let report = device.core.run_pruning().unwrap();
    assert_eq!(report.retired_versions.len(), 1);
    assert_eq!(
        report.remote_gc.unwrap().reclaimable_bytes,
        b"second".len() as u64
    );
    let catalog = sync::load_catalog(remote.path()).unwrap();
    assert_eq!(catalog.games[0].slots[0].versions.len(), 4);
    assert_eq!(catalog.retired_versions.len(), 1);
}

#[test]
fn concurrent_checkpoint_and_restore_remain_valid_during_cleanup() {
    let remote = tempdir().unwrap();
    let device = DeviceHarness::new(remote.path());
    device
        .core
        .update_sync_settings(SyncSettings { remote_path: None })
        .unwrap();
    let mut game = device.add_game("Concurrent Cleanup", b"first");
    for bytes in [b"second".as_slice(), b"third", b"fourth"] {
        device.write(bytes);
        game = device.checkpoint(game.game.id);
    }
    let first_id = game
        .versions
        .iter()
        .find(|v| v.version_number == 1)
        .unwrap()
        .id;
    device.write(b"fifth");
    let repository_root = device.data.path().join("repository");
    let core = &device.core;
    let versions = &device.versions;
    let current = core.get_game(game.game.id).unwrap();
    std::thread::scope(|scope| {
        let cleanup = scope.spawn(|| core.run_pruning().unwrap());
        let checkpoint = scope.spawn(|| {
            checkpoint::checkpoint_current_state(
                &repository_root,
                versions.as_ref(),
                &current,
                VersionReason::Manual,
            )
            .unwrap()
        });
        cleanup.join().unwrap();
        assert!(matches!(
            checkpoint.join().unwrap(),
            CheckpointOutcome::Created(_)
        ));
    });
    assert_eq!(core.get_game(game.game.id).unwrap().versions.len(), 5);
    std::thread::scope(|scope| {
        let cleanup = scope.spawn(|| core.run_pruning().unwrap());
        let restore = scope.spawn(|| core.restore_version(first_id).unwrap());
        cleanup.join().unwrap();
        restore.join().unwrap();
    });
    assert_eq!(device.bytes(), b"first");
}

#[test]
fn removed_device_cannot_reappear_from_delayed_cloud_files() {
    let remote = tempdir().unwrap();
    let first = DeviceHarness::new(remote.path());
    let second = DeviceHarness::new(remote.path());
    first.core.push_sync().unwrap();
    second.core.push_sync().unwrap();
    let second_uuid = second.core.device_metadata().device_uuid;
    let device_file = sync::remote_device_file(remote.path(), second_uuid);
    let state_file = sync::remote_state_file(remote.path(), second_uuid);
    let device_bytes = fs::read(&device_file).unwrap();
    let state_bytes = fs::read(&state_file).unwrap();
    fs::remove_file(&state_file).unwrap();
    assert!(
        first.core.run_pruning().is_err(),
        "missing device state must block GC"
    );
    fs::write(&state_file, &state_bytes).unwrap();
    assert!(
        first.core.run_pruning().is_err(),
        "registered offline device must block GC"
    );
    first.core.remove_sync_device(second_uuid).unwrap();
    fs::create_dir_all(device_file.parent().unwrap()).unwrap();
    fs::write(&device_file, device_bytes).unwrap();
    fs::write(&state_file, state_bytes).unwrap();
    assert_eq!(sync::load_catalog(remote.path()).unwrap().devices.len(), 1);
    assert!(second.core.push_sync().is_err());
    assert!(first.core.run_pruning().is_ok());
}

#[test]
fn tagging_a_published_version_preserves_a_remote_retention_anchor() {
    let remote = tempdir().unwrap();
    let device = DeviceHarness::new(remote.path());
    let game = device.add_game("Tagged Game", b"save");
    device.core.push_sync().unwrap();
    let version = game.last_version.unwrap();
    device.core.tag_version(version.id, "keep".into()).unwrap();
    device.core.push_sync().unwrap();
    let catalog = sync::load_catalog(remote.path()).unwrap();
    assert_eq!(
        catalog.games[0].slots[0].versions[0].version.tag.as_deref(),
        Some("keep")
    );
    device.core.tag_version(version.id, "".into()).unwrap();
    device.core.push_sync().unwrap();
    let catalog = sync::load_catalog(remote.path()).unwrap();
    assert_eq!(
        catalog.games[0].slots[0].versions[0].version.tag.as_deref(),
        Some("keep")
    );
}

#[test]
fn two_repositories_fast_forward_adopt_and_preserve_genuine_divergence() {
    let remote = tempdir().unwrap();
    let first = DeviceHarness::new(remote.path());
    let first_game = first.add_game("Sync Harness Game", b"v1");
    let game_uuid = Uuid::parse_str(first_game.game.uuid.as_deref().unwrap()).unwrap();
    first.core.push_sync().unwrap();
    let remote_catalog = sync::load_catalog(remote.path()).unwrap();
    let remote_game = &remote_catalog.games[0];
    let remote_game_dir = repository::game_dir(remote.path(), &remote_game.metadata.folder_slug);
    let remote_slot_dir =
        repository::slot_dir(&remote_game_dir, &remote_game.slots[0].metadata.folder_slug);
    assert!(
        !fs::read_to_string(repository::metadata_path(&remote_game_dir))
            .unwrap()
            .contains("active_slot_uuid")
    );
    assert!(
        !fs::read_to_string(repository::metadata_path(&remote_slot_dir))
            .unwrap()
            .contains("active_version_uuid")
    );

    let second = DeviceHarness::new(remote.path());
    point_remote_tracked_dir_at(remote.path(), game_uuid, second.live.path());
    let adopted = second.core.adopt_remote_game(game_uuid).unwrap();
    assert_eq!(second.bytes(), b"v1");
    assert_eq!(adopted.versions.len(), 1);

    first.write(b"v2");
    first.checkpoint(first_game.game.id);
    first.core.push_sync().unwrap();
    let report = second.core.refresh_sync().unwrap();
    assert_eq!(
        second.core.get_game(adopted.game.id).unwrap().tracked_dirs[0].path,
        second.live.path().to_string_lossy(),
        "remote metadata must not replace this device's save location"
    );
    let status = report
        .games
        .iter()
        .find(|status| status.game_uuid == game_uuid)
        .unwrap();
    assert_eq!(status.relation, Divergence::FastForward);
    assert!(
        !status.auto_applied,
        "unknown install path must fail closed"
    );
    second
        .core
        .restore_version(status.remote_version_id.unwrap())
        .unwrap();
    assert_eq!(second.bytes(), b"v2");

    second.write(b"second branch");
    first.write(b"first branch");
    first.checkpoint(first_game.game.id);
    first.core.push_sync().unwrap();

    let report = second.core.refresh_sync().unwrap();
    let status = report
        .games
        .iter()
        .find(|status| status.game_uuid == game_uuid)
        .unwrap();
    assert_eq!(status.relation, Divergence::Diverged);
    assert_eq!(
        second
            .core
            .get_game(adopted.game.id)
            .unwrap()
            .versions
            .len(),
        4
    );
    assert_eq!(
        second.bytes(),
        b"second branch",
        "unversioned progress must be checkpointed before sync and never overwritten"
    );
}

#[test]
fn adoption_preserves_existing_local_saves() {
    let remote = tempdir().unwrap();
    let first = DeviceHarness::new(remote.path());
    let first_game = first.add_game("Existing Save Game", b"remote progress");
    let game_uuid = Uuid::parse_str(first_game.game.uuid.as_deref().unwrap()).unwrap();
    first.core.push_sync().unwrap();

    let second = DeviceHarness::new(remote.path());
    second.write(b"local progress");
    point_remote_tracked_dir_at(remote.path(), game_uuid, second.live.path());
    let adopted = second.core.adopt_remote_game(game_uuid).unwrap();
    assert_eq!(second.bytes(), b"local progress");
    let local_version = adopted
        .versions
        .iter()
        .find(|version| Some(version.id) == adopted.active_slot.as_ref().unwrap().active_version_id)
        .unwrap();
    assert!(
        local_version.uuid.is_some(),
        "existing progress must be checkpointed"
    );
    assert_eq!(adopted.versions.len(), 2);
}

#[test]
fn divergence_uses_active_version_after_rollback() {
    let remote = tempdir().unwrap();
    let first = DeviceHarness::new(remote.path());
    let first_game = first.add_game("Rollback Game", b"base");
    let game_uuid = Uuid::parse_str(first_game.game.uuid.as_deref().unwrap()).unwrap();
    first.core.push_sync().unwrap();

    let second = DeviceHarness::new(remote.path());
    point_remote_tracked_dir_at(remote.path(), game_uuid, second.live.path());
    let adopted = second.core.adopt_remote_game(game_uuid).unwrap();
    let base_id = adopted.active_slot.unwrap().active_version_id.unwrap();
    second.write(b"local branch");
    second.checkpoint(adopted.game.id);
    second.core.push_sync().unwrap();
    first.write(b"remote branch");
    first.checkpoint(first_game.game.id);
    first.core.push_sync().unwrap();

    second.core.restore_version(base_id).unwrap();
    let report = second.core.refresh_sync().unwrap();
    assert_eq!(report.games[0].relation, Divergence::FastForward);
}

#[test]
fn newest_remote_head_wins_over_a_later_idle_device_refresh() {
    let remote = tempdir().unwrap();
    let first = DeviceHarness::new(remote.path());
    let first_game = first.add_game("Three Device Game", b"base");
    let game_uuid = Uuid::parse_str(first_game.game.uuid.as_deref().unwrap()).unwrap();
    first.core.push_sync().unwrap();
    let second = DeviceHarness::new(remote.path());
    point_remote_tracked_dir_at(remote.path(), game_uuid, second.live.path());
    second.core.adopt_remote_game(game_uuid).unwrap();
    first.write(b"newer");
    let newer = first.checkpoint(first_game.game.id);
    first.core.push_sync().unwrap();
    second.core.push_sync().unwrap();

    let catalog = sync::load_catalog(remote.path()).unwrap();
    let tip = sync::latest_remote_tip(&catalog, Uuid::new_v4(), game_uuid).unwrap();
    assert_eq!(
        tip.version_uuid.unwrap().to_string(),
        newer.last_version.unwrap().uuid.unwrap()
    );
    assert!(!sync::remote_heads_diverge(
        &catalog,
        Uuid::new_v4(),
        game_uuid
    ));
}

#[test]
fn manual_apply_checkpoints_unsaved_local_progress() {
    let remote = tempdir().unwrap();
    let first = DeviceHarness::new(remote.path());
    let first_game = first.add_game("Manual Apply Game", b"base");
    let game_uuid = Uuid::parse_str(first_game.game.uuid.as_deref().unwrap()).unwrap();
    first.core.push_sync().unwrap();
    let second = DeviceHarness::new(remote.path());
    point_remote_tracked_dir_at(remote.path(), game_uuid, second.live.path());
    let adopted = second.core.adopt_remote_game(game_uuid).unwrap();
    first.write(b"remote progress");
    first.checkpoint(first_game.game.id);
    first.core.push_sync().unwrap();
    let report = second.core.refresh_sync().unwrap();
    let remote_id = report.games[0].remote_version_id.unwrap();
    second.write(b"unsaved local progress");
    second.core.apply_remote_version(remote_id).unwrap();
    assert_eq!(second.bytes(), b"remote progress");
    assert_eq!(
        second
            .core
            .get_game(adopted.game.id)
            .unwrap()
            .versions
            .len(),
        3
    );
}

#[test]
fn sync_rejects_local_repository_overlap() {
    let remote = tempdir().unwrap();
    let device = DeviceHarness::new(remote.path());
    let repository = device.data.path().join("repository");
    for path in [device.data.path(), repository.as_path()] {
        assert!(
            device
                .core
                .update_sync_settings(SyncSettings {
                    remote_path: Some(path.to_string_lossy().into_owned()),
                })
                .is_err()
        );
    }
    assert!(
        sync::push(
            &repository,
            &repository,
            &device.core.device_metadata(),
            &sync::DeviceState {
                device_uuid: device.core.device_metadata().device_uuid,
                updated_at: chrono::Utc::now(),
                games: HashMap::new()
            },
        )
        .is_err()
    );
}

fn point_remote_tracked_dir_at(remote: &Path, game_uuid: Uuid, live: &Path) {
    let catalog = sync::load_catalog(remote).unwrap();
    let game = catalog
        .games
        .iter()
        .find(|game| game.metadata.uuid == game_uuid)
        .unwrap();
    let game_dir = repository::game_dir(remote, &game.metadata.folder_slug);
    let tracked = &game.tracked_dirs[0];
    let tracked_dir = repository::tracked_dir_dir(&game_dir, &tracked.folder_slug);
    let mut metadata: TrackedDirMetadata = repository::read_metadata_json(&tracked_dir).unwrap();
    metadata.path = live.to_string_lossy().into_owned();
    repository::write_metadata_json(&tracked_dir, &metadata).unwrap();
}

#[test]
fn concurrent_identical_blob_pushes_are_idempotent() {
    let source = tempdir().unwrap();
    let remote = tempdir().unwrap();
    let first = DeviceHarness::new(source.path());
    first.add_game("Concurrent Blob Game", b"same bytes");
    let device = first.core.device_metadata();
    let state = sync::DeviceState {
        device_uuid: device.device_uuid,
        updated_at: chrono::Utc::now(),
        games: HashMap::new(),
    };
    let repository = first.data.path().join("repository");
    std::thread::scope(|scope| {
        let left = scope.spawn(|| {
            sync::push(&repository, remote.path(), &device, &state).unwrap();
        });
        let right = scope.spawn(|| {
            sync::push(&repository, remote.path(), &device, &state).unwrap();
        });
        left.join().unwrap();
        right.join().unwrap();
    });

    let catalog = sync::load_catalog(remote.path()).unwrap();
    let version = &catalog.games[0].slots[0].versions[0];
    let file = &version.version.files[0];
    assert!(
        savegame_timeline_core::content_store::verify_blob(remote.path(), &file.sha256, file.size)
            .is_ok()
    );
}
