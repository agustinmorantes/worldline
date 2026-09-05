use core::time::Duration;
use diesel::SqliteConnection;
use diesel::r2d2::{ConnectionManager, Pool};
use diesel_migrations::MigrationHarness;
use savegame_timeline_core::MIGRATIONS;
use savegame_timeline_core::TimelineCore;
use savegame_timeline_core::models::*;
use savegame_timeline_core::persistence::sqlite::{
    SqliteGameRepository, SqliteScanRootsRepository, SqliteVersionRepository,
};
use std::fs;
use std::sync::Arc;
use tempfile::tempdir;

#[test]
fn test_restore_uses_version_id_not_version_number() {
    // Regression: restore_version must use version.id (the DB primary key) as the
    // snapshot directory name, not version_number (the per-game sequential counter).
    // For Game 2's first version, version.id=2 but version_number=1.
    // The snapshot is stored under working_base/{game_id}/{version.id}/{dir_id},
    // so using version_number instead would look in the wrong directory.
    let pool = init_db();
    let games_repo = Arc::new(SqliteGameRepository::new(pool.clone()));
    let versions_repo = Arc::new(SqliteVersionRepository::new(pool.clone()));
    let scan_roots_repo = Arc::new(SqliteScanRootsRepository::new(pool.clone()));

    let data_dir = tempdir().unwrap();
    let working_base = data_dir.path().join("repository");
    let icons_dir = data_dir.path().join("icons");

    let game1_dir = tempdir().unwrap();
    let game2_dir = tempdir().unwrap();
    fs::write(game1_dir.path().join("save.dat"), "game1 data").unwrap();
    // game2 initial content is written before game2 is registered, so the
    // first-scan snapshot at working_base/2/1/dir captures this value.
    fs::write(game2_dir.path().join("save.dat"), "game2 initial").unwrap();

    let (core, notif_rx) = TimelineCore::new(
        games_repo.clone(),
        versions_repo.clone(),
        scan_roots_repo.clone(),
        working_base.clone(),
        icons_dir.clone(),
        Some(0.1),
    )
    .unwrap();

    let game1 = core
        .save_game(
            GameForm {
                id: None,
                uuid: None,
                title: "Game 1",
                title_normalized: None,
                folder_slug: None,
                active_slot_id: None,
                icon_path: None,
            },
            Some(vec![game1_dir.path().to_str().unwrap()]),
        )
        .unwrap();
    let game2 = core
        .save_game(
            GameForm {
                id: None,
                uuid: None,
                title: "Game 2",
                title_normalized: None,
                folder_slug: None,
                active_slot_id: None,
                icon_path: None,
            },
            Some(vec![game2_dir.path().to_str().unwrap()]),
        )
        .unwrap();

    // Let the watcher register and perform initial scans for both games.
    std::thread::sleep(Duration::from_millis(200));

    // Trigger a version for Game 1 — this consumes version id=1, so Game 2's
    // first version will get id=2 (while its version_number will be 1).
    fs::write(game1_dir.path().join("save.dat"), "game1 modified").unwrap();
    loop {
        std::thread::sleep(Duration::from_millis(300));
        let g = core.get_game(game1.id).unwrap();
        if !g.versions.is_empty() {
            break;
        }
    }
    while notif_rx.try_recv().is_ok() {}

    // Now write the state we want to restore to. After this, the watcher will
    // create version id=2 (version_number=1) for Game 2, snapshotting this content.
    fs::write(game2_dir.path().join("save.dat"), "game2 good save").unwrap();
    loop {
        std::thread::sleep(Duration::from_millis(300));
        let g = core.get_game(game2.id).unwrap();
        if !g.versions.is_empty() {
            break;
        }
    }
    while notif_rx.try_recv().is_ok() {}

    let full_game2 = core.get_game(game2.id).unwrap();
    let version = full_game2
        .versions
        .last()
        .expect("Game 2 should have at least one version");

    // Precondition: this is the exact mismatch the bug exploited.
    assert_eq!(version.version_number, 1, "Game 2's first version_number should be 1");
    assert!(
        version.id > version.version_number,
        "Game 2's version id ({}) should be greater than its version_number ({}) \
         because Game 1's version consumed id=1 first",
        version.id,
        version.version_number
    );

    // Corrupt the save file and immediately restore (before the 0.1s debounce fires).
    fs::write(game2_dir.path().join("save.dat"), "corrupted!").unwrap();
    core.restore_version(version.id).unwrap();

    let restored = fs::read_to_string(game2_dir.path().join("save.dat")).unwrap();
    assert_eq!(
        restored, "game2 good save",
        "restore_version should recover the snapshotted content, not the initial scan content"
    );
}

#[test]
fn test_version_cooldown_prevents_rapid_second_version() {
    // Regression: after creating a version, the watcher's in-memory games snapshot was not
    // refreshed, so the cooldown check on the next debounce would see 0 versions and
    // immediately create a second version.
    let pool = init_db();
    let games_repo = Arc::new(SqliteGameRepository::new(pool.clone()));
    let versions_repo = Arc::new(SqliteVersionRepository::new(pool.clone()));
    let scan_roots_repo = Arc::new(SqliteScanRootsRepository::new(pool.clone()));

    let data_dir = tempdir().unwrap();
    let source_dir = tempdir().unwrap();
    let working_base = data_dir.path().join("repository");
    let icons_dir = data_dir.path().join("icons");
    let save_file = source_dir.path().join("save.dat");
    fs::write(&save_file, "initial").unwrap();

    let (core, notif_rx) = TimelineCore::new(
        games_repo,
        versions_repo,
        scan_roots_repo,
        working_base,
        icons_dir,
        Some(0.1),
    )
    .unwrap();

    let game = core
        .save_game(
            GameForm {
                id: None,
                uuid: None,
                title: "Cooldown Test",
                title_normalized: None,
                folder_slug: None,
                active_slot_id: None,
                icon_path: None,
            },
            Some(vec![source_dir.path().to_str().unwrap()]),
        )
        .unwrap();

    // Give the watcher time to register the directory before writing.
    std::thread::sleep(Duration::from_millis(100));

    // Trigger first change and wait for version 1 to be created.
    fs::write(&save_file, "change 1").unwrap();
    loop {
        std::thread::sleep(Duration::from_millis(300));
        if !core.get_game(game.id).unwrap().versions.is_empty() {
            break;
        }
    }
    while notif_rx.try_recv().is_ok() {}

    // Trigger a second change immediately — still well within the 15-minute cooldown.
    fs::write(&save_file, "change 2").unwrap();

    // Wait long enough for the debounce to fire (0.1s) plus processing, but not
    // nearly long enough for the cooldown to expire.
    std::thread::sleep(Duration::from_millis(800));

    let full_game = core.get_game(game.id).unwrap();
    assert_eq!(
        full_game.versions.len(),
        1,
        "Second change within the cooldown window must not create a new version"
    );
}

fn init_db() -> Pool<ConnectionManager<SqliteConnection>> {
    let manager = ConnectionManager::<SqliteConnection>::new(":memory:");
    let pool = Pool::builder().max_size(1).build(manager).unwrap();
    let mut conn = pool.get().unwrap();
    conn.run_pending_migrations(MIGRATIONS).unwrap();
    pool
}

#[test]
fn test_full_e2e_flow() {
    let pool = init_db();
    let working_base = tempdir().unwrap();
    let games_repo = Arc::new(SqliteGameRepository::with_repository_root(
        pool.clone(),
        working_base.path().to_path_buf(),
    ));
    let versions_repo = Arc::new(SqliteVersionRepository::with_repository_root(
        pool.clone(),
        working_base.path().to_path_buf(),
    ));
    let scan_roots_repo = Arc::new(SqliteScanRootsRepository::new(pool.clone()));
    let source_dir = tempdir().unwrap();

    let save_file = source_dir.path().join("save.dat");
    fs::write(&save_file, "initial text").unwrap();

    let icons_dir = tempdir().unwrap();
    let (core, notif_rx) = TimelineCore::new(
        games_repo.clone(),
        versions_repo.clone(),
        scan_roots_repo.clone(),
        working_base.path().to_path_buf(),
        icons_dir.path().to_path_buf(),
        Some(0.1),
    )
    .unwrap();

    let form = GameForm {
        id: None,
        uuid: None,
        title: "E2E Test",
        title_normalized: None,
        folder_slug: None,
        active_slot_id: None,
        icon_path: None,
    };
    let path_str = source_dir.path().to_str().unwrap();
    let game = core.save_game(form, Some(vec![path_str])).unwrap();

    std::thread::sleep(Duration::from_millis(50));

    fs::write(&save_file, "modified text").unwrap();

    std::thread::sleep(Duration::from_millis(2500));

    let mut version_created = false;
    let mut change_detected = false;

    while let Ok(msg) = notif_rx.try_recv() {
        match msg {
            savegame_timeline_core::detection::Notification::VersionCreated {
                game_id,
                version_number,
            } => {
                assert_eq!(game_id, game.id);
                assert_eq!(version_number, 1);
                version_created = true;
            }
            savegame_timeline_core::detection::Notification::ChangeDetected { game_id } => {
                assert_eq!(game_id, game.id);
                change_detected = true;
            }
            _ => {}
        }
    }

    assert!(version_created, "Version was not created automatically");
    assert!(change_detected, "Change was not detected automatically");

    let full_game = core.get_game(game.id).unwrap();
    let version_folder = savegame_timeline_core::repository::game_snapshot_dir(
        working_base.path(),
        &full_game.game,
        full_game.active_slot.as_ref().unwrap(),
        full_game.versions.last().unwrap(),
        &full_game.tracked_dirs[0],
    )
    .unwrap();

    let copied_file = version_folder.join("save.dat");
    assert!(copied_file.exists());
    let content = fs::read_to_string(copied_file).unwrap();
    assert_eq!(content, "modified text");

    // Tag version test via Facade
    let version_id = full_game.versions.last().unwrap().id;
    let tagged = core
        .tag_version(version_id, "test_tag".to_string())
        .unwrap();
    assert_eq!(tagged.tag.unwrap(), "test_tag");

    // Restore version test via Facade
    // Overwrite the original file with something else
    fs::write(&save_file, "corrupted game!").unwrap();

    // Restore the correct version
    core.restore_version(version_id).unwrap();

    // Should be restored
    let restored_content = fs::read_to_string(&save_file).unwrap();
    assert_eq!(restored_content, "modified text");
}

#[test]
fn test_negative_facade_paths() {
    let pool = init_db();
    let working_base = tempdir().unwrap();
    let games_repo = Arc::new(SqliteGameRepository::with_repository_root(
        pool.clone(),
        working_base.path().to_path_buf(),
    ));
    let versions_repo = Arc::new(SqliteVersionRepository::with_repository_root(
        pool.clone(),
        working_base.path().to_path_buf(),
    ));
    let scan_roots_repo = Arc::new(SqliteScanRootsRepository::new(pool.clone()));
    let icons_dir = tempdir().unwrap();

    let (core, _notif_rx) = TimelineCore::new(
        games_repo,
        versions_repo,
        scan_roots_repo,
        working_base.path().to_path_buf(),
        icons_dir.path().to_path_buf(),
        Some(0.0),
    )
    .unwrap();

    // restore bad version
    let res = core.restore_version(9999);
    assert!(
        res.is_err(),
        "Expected error when restoring non-existent version"
    );
}

#[test]
fn test_slot_creation_and_switching_preserve_independent_histories() {
    let pool = init_db();
    let working_base = tempdir().unwrap();
    let games_repo = Arc::new(SqliteGameRepository::with_repository_root(
        pool.clone(),
        working_base.path().to_path_buf(),
    ));
    let versions_repo = Arc::new(SqliteVersionRepository::with_repository_root(
        pool.clone(),
        working_base.path().to_path_buf(),
    ));
    let scan_roots_repo = Arc::new(SqliteScanRootsRepository::new(pool.clone()));
    let source_dir = tempdir().unwrap();
    let icons_dir = tempdir().unwrap();

    let save_file = source_dir.path().join("save.dat");
    fs::write(&save_file, "slot-one-state").unwrap();

    let (core, _notif_rx) = TimelineCore::new(
        games_repo.clone(),
        versions_repo.clone(),
        scan_roots_repo,
        working_base.path().to_path_buf(),
        icons_dir.path().to_path_buf(),
        Some(0.1),
    )
    .unwrap();

    let game = core
        .save_game(
            GameForm {
                id: None,
                uuid: None,
                title: "Slot Switch Test",
                title_normalized: None,
                folder_slug: None,
                active_slot_id: None,
                icon_path: None,
            },
            Some(vec![source_dir.path().to_str().unwrap()]),
        )
        .unwrap();

    let game_with_new_slot = core.create_slot(game.id, "Second Slot").unwrap();
    assert_eq!(
        game_with_new_slot.active_slot.as_ref().unwrap().name,
        "Second Slot"
    );
    assert_eq!(fs::read_dir(source_dir.path()).unwrap().count(), 0);

    fs::write(&save_file, "slot-two-state").unwrap();

    let default_slot_id = game_with_new_slot
        .slots
        .iter()
        .find(|slot| slot.name == "Default Slot")
        .unwrap()
        .id;
    let second_slot_id = game_with_new_slot.active_slot.as_ref().unwrap().id;

    let default_game = core.switch_slot(game.id, default_slot_id).unwrap();
    assert_eq!(
        default_game.active_slot.as_ref().unwrap().name,
        "Default Slot"
    );
    assert_eq!(fs::read_to_string(&save_file).unwrap(), "slot-one-state");

    let second_game = core.switch_slot(game.id, second_slot_id).unwrap();
    assert_eq!(
        second_game.active_slot.as_ref().unwrap().name,
        "Second Slot"
    );
    assert_eq!(fs::read_to_string(&save_file).unwrap(), "slot-two-state");
}

/// Everything a facade test needs. Fields are kept alive by the struct:
/// dropping a `TempDir` deletes it, so these must outlive the test body.
#[allow(dead_code)]
struct Harness {
    core: TimelineCore,
    pool: Pool<ConnectionManager<SqliteConnection>>,
    working_base: tempfile::TempDir,
    source_dir: tempfile::TempDir,
    _icons_dir: tempfile::TempDir,
    save_file: std::path::PathBuf,
}

#[allow(dead_code)]
fn harness(initial_contents: &str) -> Harness {
    let pool = init_db();
    let working_base = tempdir().unwrap();
    let source_dir = tempdir().unwrap();
    let icons_dir = tempdir().unwrap();

    let save_file = source_dir.path().join("save.dat");
    fs::write(&save_file, initial_contents).unwrap();

    let games_repo = Arc::new(SqliteGameRepository::with_repository_root(
        pool.clone(),
        working_base.path().to_path_buf(),
    ));
    let versions_repo = Arc::new(SqliteVersionRepository::with_repository_root(
        pool.clone(),
        working_base.path().to_path_buf(),
    ));
    let scan_roots_repo = Arc::new(SqliteScanRootsRepository::new(pool.clone()));

    let (core, _rx) = TimelineCore::new(
        games_repo,
        versions_repo,
        scan_roots_repo,
        working_base.path().to_path_buf(),
        icons_dir.path().to_path_buf(),
        Some(0.1),
    )
    .unwrap();

    Harness {
        core,
        pool,
        working_base,
        source_dir,
        _icons_dir: icons_dir,
        save_file,
    }
}

#[allow(dead_code)]
impl Harness {
    /// Creates a game tracking `source_dir`, returning the created game.
    fn create_game(&self, title: &str) -> Game {
        self.core
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
                Some(vec![self.source_dir.path().to_str().unwrap()]),
            )
            .unwrap()
    }
}

#[test]
fn test_monitoring_settings_persist_through_facade() {
    let pool = init_db();
    let games_repo = Arc::new(SqliteGameRepository::new(pool.clone()));
    let versions_repo = Arc::new(SqliteVersionRepository::new(pool.clone()));
    let scan_roots_repo = Arc::new(SqliteScanRootsRepository::new(pool.clone()));

    let data_dir = tempdir().unwrap();
    let working_base = data_dir.path().join("repository");
    let icons_dir = data_dir.path().join("icons");

    let (core, _notif_rx) = TimelineCore::new(
        games_repo,
        versions_repo,
        scan_roots_repo,
        working_base.clone(),
        icons_dir,
        None,
    )
    .unwrap();

    let default_settings = core.get_monitoring_settings().unwrap();
    assert_eq!(default_settings.version_cooldown_minutes, 15);
    assert_eq!(default_settings.snapshot_debounce_seconds, 5.0);

    let updated = core
        .update_monitoring_settings(MonitoringSettings {
            version_cooldown_minutes: 22,
            snapshot_debounce_seconds: 7.0,
        })
        .unwrap();

    assert_eq!(updated.version_cooldown_minutes, 22);
    assert_eq!(updated.snapshot_debounce_seconds, 7.0);

    let reloaded = core.get_monitoring_settings().unwrap();
    assert_eq!(reloaded, updated);
}
