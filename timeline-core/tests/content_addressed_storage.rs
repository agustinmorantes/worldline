use diesel::SqliteConnection;
use diesel::r2d2::{ConnectionManager, Pool};
use diesel_migrations::MigrationHarness;
use savegame_timeline_core::MIGRATIONS;
use savegame_timeline_core::TimelineCore;
use savegame_timeline_core::checkpoint::{self, CheckpointOutcome};
use savegame_timeline_core::models::{FullGame, FullVersion, GameForm};
use savegame_timeline_core::persistence::sqlite::{
    SqliteGameRepository, SqliteScanRootsRepository, SqliteVersionRepository,
};
use savegame_timeline_core::repository::{self, VersionMetadata, VersionReason};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::{TempDir, tempdir};

const FILES_PER_VERSION: usize = 10;
const FILE_SIZE: usize = 4096;

// Nine distinct baseline blobs plus one repeated baseline intentionally give the
// fixture ten files but only nine baseline contents. The five changed contents
// (1..=5) are disjoint from these literals, for fourteen blobs total.
const INITIAL_BYTES: [u8; FILES_PER_VERSION] =
    [0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7, 0xA8, 0xA0];

fn init_db() -> Pool<ConnectionManager<SqliteConnection>> {
    let manager = ConnectionManager::<SqliteConnection>::new(":memory:");
    let pool = Pool::builder().max_size(1).build(manager).unwrap();
    let mut conn = pool.get().unwrap();
    conn.run_pending_migrations(MIGRATIONS).unwrap();
    pool
}

fn file_name(index: usize) -> String {
    format!("save-{index:02}.dat")
}

fn expected_bytes(version_index: usize) -> Vec<Vec<u8>> {
    let mut expected = INITIAL_BYTES
        .into_iter()
        .map(|byte| vec![byte; FILE_SIZE])
        .collect::<Vec<_>>();
    for change in 0..=version_index {
        expected[change % FILES_PER_VERSION] = vec![(change + 1) as u8; FILE_SIZE];
    }
    expected
}

struct CasHarness {
    core: TimelineCore,
    versions_repo: Arc<SqliteVersionRepository>,
    game: FullGame,
    repository_root: PathBuf,
    file_count: usize,
    file_size: usize,
    live_dir: TempDir,
    _data_dir: TempDir,
    _icons_dir: TempDir,
    _notif_rx: savegame_timeline_core::detection::NotificationReceiver,
}

impl CasHarness {
    fn new(file_count: usize, file_size: usize) -> Self {
        assert_eq!(file_count, FILES_PER_VERSION);
        assert_eq!(file_size, FILE_SIZE);

        let pool = init_db();
        let data_dir = tempdir().unwrap();
        let repository_root = data_dir.path().join("repository");
        let live_dir = tempdir().unwrap();
        let icons_dir = tempdir().unwrap();

        for (index, byte) in INITIAL_BYTES.into_iter().enumerate() {
            fs::write(
                live_dir.path().join(file_name(index)),
                vec![byte; file_size],
            )
            .unwrap();
        }

        let games_repo = Arc::new(SqliteGameRepository::with_repository_root(
            pool.clone(),
            repository_root.clone(),
        ));
        let versions_repo = Arc::new(SqliteVersionRepository::with_repository_root(
            pool.clone(),
            repository_root.clone(),
        ));
        let scan_roots_repo = Arc::new(SqliteScanRootsRepository::new(pool));
        let (core, notif_rx) = TimelineCore::new(
            games_repo,
            versions_repo.clone(),
            scan_roots_repo,
            repository_root.clone(),
            icons_dir.path().to_path_buf(),
            Some(5.0),
        )
        .unwrap();

        let game = core
            .save_game(
                GameForm {
                    id: None,
                    uuid: None,
                    title: "Content Addressed Storage Test",
                    title_normalized: None,
                    folder_slug: None,
                    active_slot_id: None,
                    icon_path: None,
                },
                Some(vec![live_dir.path().to_str().unwrap()]),
            )
            .unwrap();
        let game = core.get_game(game.id).unwrap();
        assert!(
            game.last_version.is_none(),
            "fixture setup must not create a version before checkpointing"
        );

        Self {
            core,
            versions_repo,
            game,
            repository_root,
            file_count,
            file_size,
            live_dir,
            _data_dir: data_dir,
            _icons_dir: icons_dir,
            _notif_rx: notif_rx,
        }
    }

    fn change_one_file(&self, n: usize) {
        let index = n % self.file_count;
        fs::write(
            self.live_dir.path().join(file_name(index)),
            vec![(n + 1) as u8; self.file_size],
        )
        .unwrap();
    }

    fn checkpoint(&mut self) -> CheckpointOutcome {
        self.game = self.core.get_game(self.game.game.id).unwrap();
        let outcome = checkpoint::checkpoint_current_state(
            &self.repository_root,
            self.versions_repo.as_ref(),
            &self.game,
            VersionReason::Manual,
        )
        .unwrap();
        self.game = self.core.get_game(self.game.game.id).unwrap();
        outcome
    }

    fn total_blob_bytes(&self) -> u64 {
        regular_file_stats(&self.repository_root.join("blobs")).0
    }

    fn count_blob_files(&self) -> usize {
        regular_file_stats(&self.repository_root.join("blobs")).1
    }

    fn count_legacy_dirs(&self) -> usize {
        count_named_directories(&self.repository_root, "dirs")
    }

    fn version_metadata(&self, version: &FullVersion) -> VersionMetadata {
        let game_folder_slug = version.game.folder_slug.as_deref().unwrap();
        let version_folder_slug = version.version.folder_slug.as_deref().unwrap();
        let game_dir = repository::game_dir(&self.repository_root, game_folder_slug);
        let slot_dir = repository::slot_dir(&game_dir, &version.slot.folder_slug);
        let version_dir = repository::version_dir(&slot_dir, version_folder_slug);
        repository::read_metadata_json(&version_dir).unwrap()
    }

    fn restore_and_read_all(&self, version_id: i32) -> Vec<Vec<u8>> {
        self.core.restore_version(version_id).unwrap();
        (0..self.file_count)
            .map(|index| fs::read(self.live_dir.path().join(file_name(index))).unwrap())
            .collect()
    }
}

impl Drop for CasHarness {
    fn drop(&mut self) {
        self.core.shutdown();
    }
}

fn regular_file_stats(root: &Path) -> (u64, usize) {
    if !root.exists() {
        return (0, 0);
    }

    fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            let metadata = fs::symlink_metadata(&path).unwrap();
            if metadata.is_dir() {
                regular_file_stats(&path)
            } else if metadata.is_file() {
                (metadata.len(), 1)
            } else {
                (0, 0)
            }
        })
        .fold((0, 0), |(bytes, files), (entry_bytes, entry_files)| {
            (bytes + entry_bytes, files + entry_files)
        })
}

fn count_named_directories(root: &Path, name: &str) -> usize {
    if !root.exists() {
        return 0;
    }

    fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            let metadata = fs::symlink_metadata(&path).unwrap();
            if !metadata.is_dir() {
                return 0;
            }
            usize::from(path.file_name().and_then(|value| value.to_str()) == Some(name))
                + count_named_directories(&path, name)
        })
        .sum()
}

#[test]
fn representative_save_set_uses_less_than_half_the_full_copy_bytes() {
    const VERSION_COUNT: usize = 5;
    let mut harness = CasHarness::new(FILES_PER_VERSION, FILE_SIZE);
    let mut version_ids = Vec::new();
    let mut content_hashes = Vec::new();

    for version_index in 0..VERSION_COUNT {
        harness.change_one_file(version_index);
        let CheckpointOutcome::Created(version) = harness.checkpoint() else {
            panic!("each changed save set must create exactly one version");
        };
        version_ids.push(version.version.id);
        content_hashes.push(harness.version_metadata(&version).content_hash);
        assert_eq!(harness.game.versions.len(), version_index + 1);
    }

    assert_eq!(version_ids.len(), VERSION_COUNT);
    assert_eq!(
        version_ids
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        VERSION_COUNT,
        "each changed save set must receive a distinct version id"
    );
    assert_eq!(
        content_hashes
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        VERSION_COUNT,
        "each changed save set must receive a distinct manifest"
    );

    let CheckpointOutcome::Unchanged(unchanged) = harness.checkpoint() else {
        panic!("repeating an unchanged save set must not create a duplicate version");
    };
    assert_eq!(unchanged.version.id, *version_ids.last().unwrap());
    assert_eq!(harness.game.versions.len(), VERSION_COUNT);

    let full_copy_bytes = (FILES_PER_VERSION * VERSION_COUNT * FILE_SIZE) as u64;
    let blob_bytes = harness.total_blob_bytes();
    let ratio = blob_bytes as f64 / full_copy_bytes as f64;
    eprintln!(
        "content-addressed storage: {}/{} bytes, {} unique blobs, ratio = {ratio:.3}",
        blob_bytes,
        full_copy_bytes,
        harness.count_blob_files()
    );
    assert_eq!(harness.count_blob_files(), 14);
    assert_eq!(blob_bytes, 14 * FILE_SIZE as u64);
    assert!(
        ratio < 0.50,
        "storage ratio {ratio:.3} must remain below 0.50"
    );
    assert_eq!(harness.count_legacy_dirs(), 0);

    let first = harness.restore_and_read_all(version_ids[0]);
    assert_eq!(first, expected_bytes(0));
    let last = harness.restore_and_read_all(*version_ids.last().unwrap());
    assert_eq!(last, expected_bytes(VERSION_COUNT - 1));
}
