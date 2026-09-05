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
