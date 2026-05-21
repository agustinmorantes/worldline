use diesel::SqliteConnection;
use diesel::r2d2::{ConnectionManager, Pool};
use diesel_migrations::MigrationHarness;
use savegame_timeline_core::MIGRATIONS;
use savegame_timeline_core::models::*;
use savegame_timeline_core::persistence::sqlite::{SqliteGameRepository, SqliteVersionRepository};
use savegame_timeline_core::persistence::{GameRepository, VersionRepository};
use savegame_timeline_core::repository;
use tempfile::tempdir;

fn init_db() -> Pool<ConnectionManager<SqliteConnection>> {
    let manager = ConnectionManager::<SqliteConnection>::new(":memory:");
    // Pin to 1 connection to avoid multiple memory databases
    let pool = Pool::builder().max_size(1).build(manager).unwrap();
    let mut conn = pool.get().unwrap();
    conn.run_pending_migrations(MIGRATIONS).unwrap();
    pool
}

#[test]
fn test_game_crud() {
    let pool = init_db();
    let games_repo = SqliteGameRepository::new(pool.clone());

    // Create
    let form = GameForm {
        id: None,
        uuid: None,
        title: "Integration Test Game",
        title_normalized: None,
        folder_slug: None,
        active_version_id: None,
        active_slot_id: None,
        icon_path: None,
    };

    let game = games_repo
        .save_game(form, Some(vec!["/test/path"]))
        .unwrap();
    assert_eq!(game.title, "Integration Test Game");
    assert!(game.uuid.as_deref().is_some_and(|value| !value.is_empty()));
    assert_eq!(
        game.title_normalized.as_deref(),
        Some("integration test game")
    );
    assert!(
        game.folder_slug
            .as_deref()
            .is_some_and(|value| value.starts_with("integration-test-game__g_"))
    );

    // Read
    let full_game = games_repo.get_game(game.id).unwrap();
    assert_eq!(full_game.tracked_dirs.len(), 1);
    assert_eq!(full_game.tracked_dirs[0].path, "/test/path");
    assert!(
        full_game.tracked_dirs[0]
            .uuid
            .as_deref()
            .is_some_and(|value| !value.is_empty())
    );
    assert!(
        full_game.tracked_dirs[0]
            .folder_slug
            .as_deref()
            .is_some_and(|value| value.contains("__d_"))
    );

    // Update
    let update_form = GameForm {
        id: Some(game.id),
        uuid: None,
        title: "Updated Game",
        title_normalized: None,
        folder_slug: None,
        active_version_id: None,
        active_slot_id: None,
        icon_path: None,
    };
    let updated = games_repo
        .save_game(update_form, Some(vec!["/new/path"]))
        .unwrap();
    assert_eq!(updated.title, "Updated Game");

    let new_full = games_repo.get_game(game.id).unwrap();
    assert_eq!(new_full.tracked_dirs.len(), 1);
    assert_eq!(new_full.tracked_dirs[0].path, "/new/path");

    // test get_games
    let all_games = games_repo.get_games().unwrap();
    assert_eq!(all_games.len(), 1);
    assert_eq!(all_games[0].game.title, "Updated Game");

    // Delete
    games_repo.delete_game(game.id).unwrap();
    assert!(games_repo.get_game(game.id).is_err());

    let all_games_after = games_repo.get_games().unwrap();
    assert_eq!(all_games_after.len(), 0);
}

#[test]
fn test_version_operations() {
    let pool = init_db();
    let games_repo = SqliteGameRepository::new(pool.clone());
    let versions_repo = SqliteVersionRepository::new(pool.clone());

    // Setup game
    let form = GameForm {
        id: None,
        uuid: None,
        title: "Game",
        title_normalized: None,
        folder_slug: None,
        active_version_id: None,
        active_slot_id: None,
        icon_path: None,
    };
    let game = games_repo.save_game(form, None).unwrap();

    // Create version
    let version = versions_repo.create_version(game.id).unwrap();
    assert_eq!(version.version.version_number, 1);
    assert!(
        version
            .version
            .uuid
            .as_deref()
            .is_some_and(|value| !value.is_empty())
    );
    assert!(
        version
            .version
            .folder_slug
            .as_deref()
            .is_some_and(|value| value.starts_with("v001__v_"))
    );

    // Check if game was updated with activeversion
    let g = games_repo.get_game(game.id).unwrap();
    assert_eq!(g.game.active_version_id, Some(version.version.id));

    // Tag version
    let tagged = versions_repo
        .tag_version(version.version.id, "my_tag".to_string())
        .unwrap();
    assert_eq!(tagged.tag.unwrap(), "my_tag");
}

#[test]
fn test_negative_paths() {
    let pool = init_db();
    let games_repo = SqliteGameRepository::new(pool.clone());

    // get non-existent game
    let res = games_repo.get_game(9999);
    assert!(
        res.is_err(),
        "Expected error when fetching non-existent game"
    );

    // delete non-existent game
    // diesel delete with 0 affected rows doesn't error out, it returns Ok(0)
    let delete_res = games_repo.delete_game(9999);
    assert!(
        delete_res.is_ok(),
        "Deleting a non-existent game should safely return Ok"
    );
}

#[test]
fn test_repository_metadata_is_written_for_game_and_version() {
    let pool = init_db();
    let repository_root = tempdir().unwrap();
    let games_repo = SqliteGameRepository::with_repository_root(
        pool.clone(),
        repository_root.path().to_path_buf(),
    );
    let versions_repo = SqliteVersionRepository::with_repository_root(
        pool.clone(),
        repository_root.path().to_path_buf(),
    );

    let game = games_repo
        .save_game(
            GameForm {
                id: None,
                uuid: None,
                title: "Metadata Test",
                title_normalized: None,
                folder_slug: None,
                active_version_id: None,
                active_slot_id: None,
                icon_path: None,
            },
            Some(vec!["/tmp/saves"]),
        )
        .unwrap();

    let full_game = games_repo.get_game(game.id).unwrap();
    let game_dir = repository::game_dir(
        repository_root.path(),
        full_game.game.folder_slug.as_deref().unwrap(),
    );
    let game_metadata: repository::GameMetadata =
        repository::read_metadata_json(&game_dir).unwrap();
    assert_eq!(game_metadata.title, "Metadata Test");

    let tracked_dir = &full_game.tracked_dirs[0];
    let tracked_dir_dir =
        repository::tracked_dir_dir(&game_dir, tracked_dir.folder_slug.as_deref().unwrap());
    let tracked_metadata: repository::TrackedDirMetadata =
        repository::read_metadata_json(&tracked_dir_dir).unwrap();
    assert_eq!(tracked_metadata.game_uuid, game_metadata.uuid);

    let slot = full_game.active_slot.as_ref().unwrap();
    let slot_dir = repository::slot_dir(&game_dir, &slot.folder_slug);
    let slot_metadata: repository::SlotMetadata =
        repository::read_metadata_json(&slot_dir).unwrap();
    assert_eq!(slot_metadata.game_uuid, game_metadata.uuid);

    let version = versions_repo.create_version(game.id).unwrap();
    let version_dir =
        repository::version_dir(&slot_dir, version.version.folder_slug.as_deref().unwrap());
    let version_metadata: repository::VersionMetadata =
        repository::read_metadata_json(&version_dir).unwrap();
    assert_eq!(version_metadata.version_number, 1);
    assert_eq!(version_metadata.game_uuid, game_metadata.uuid);
}

#[test]
fn test_slot_repository_operations() {
    let pool = init_db();
    let games_repo = SqliteGameRepository::new(pool.clone());

    let game = games_repo
        .save_game(
            GameForm {
                id: None,
                uuid: None,
                title: "Slot Test",
                title_normalized: None,
                folder_slug: None,
                active_version_id: None,
                active_slot_id: None,
                icon_path: None,
            },
            None,
        )
        .unwrap();

    let initial_slots = games_repo.list_slots(game.id).unwrap();
    assert_eq!(initial_slots.len(), 1);
    assert_eq!(initial_slots[0].slot.name, "Default Slot");

    let second_slot = games_repo.create_slot(game.id, "Second Slot").unwrap();
    assert_eq!(second_slot.slot.name, "Second Slot");

    let slots = games_repo.list_slots(game.id).unwrap();
    assert_eq!(slots.len(), 2);

    let active_slot = games_repo
        .set_active_slot(game.id, second_slot.slot.id)
        .unwrap();
    assert_eq!(active_slot.id, second_slot.slot.id);

    let full_game = games_repo.get_game(game.id).unwrap();
    assert_eq!(
        full_game.active_slot.as_ref().unwrap().id,
        second_slot.slot.id
    );
    assert!(full_game.last_version.is_none());
}
