use diesel::SqliteConnection;
use diesel::r2d2::{ConnectionManager, Pool};
use diesel_migrations::MigrationHarness;
use savegame_timeline_core::checkpoint::{self, CheckpointOutcome};
use savegame_timeline_core::models::{FullGame, GameForm};
use savegame_timeline_core::persistence::sqlite::{
    SqliteGameRepository, SqliteScanRootsRepository, SqliteVersionRepository,
};
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
