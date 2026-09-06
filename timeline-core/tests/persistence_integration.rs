use diesel::SqliteConnection;
use diesel::r2d2::{ConnectionManager, Pool};
use diesel_migrations::MigrationHarness;
use savegame_timeline_core::MIGRATIONS;
use savegame_timeline_core::content_store::CapturedSnapshot;
use savegame_timeline_core::models::*;
use savegame_timeline_core::persistence::sqlite::{SqliteGameRepository, SqliteVersionRepository};
use savegame_timeline_core::persistence::{GameRepository, VersionRepository};
use savegame_timeline_core::repository;
use savegame_timeline_core::repository::VersionReason;
use tempfile::tempdir;

fn init_db() -> Pool<ConnectionManager<SqliteConnection>> {
    let manager = ConnectionManager::<SqliteConnection>::new(":memory:");
    // Pin to 1 connection to avoid multiple memory databases
    let pool = Pool::builder().max_size(1).build(manager).unwrap();
    let mut conn = pool.get().unwrap();
    conn.run_pending_migrations(MIGRATIONS).unwrap();
    pool
}

fn empty_snapshot() -> CapturedSnapshot {
    CapturedSnapshot {
        content_hash: String::new(),
        files: Vec::new(),
    }
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
        active_slot_id: None,
        icon_path: None,
    };
    let game = games_repo.save_game(form, None).unwrap();

    // Create version (Manual is an arbitrary placeholder here — this test
    // exercises the repository directly, not any facade operation with
    // real reason semantics)
    let version = versions_repo
        .create_version_from_snapshot(game.id, VersionReason::Manual, empty_snapshot())
        .unwrap();
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

    // Check if the active slot was updated with the active version
    // (the game-level mirror was removed by the fix_schema migration; the
    // slot's active_version_id is now the only active-version pointer).
    let g = games_repo.get_game(game.id).unwrap();
    assert_eq!(
        g.active_slot.as_ref().unwrap().active_version_id,
        Some(version.version.id)
    );

    // Tag version
    let tagged = versions_repo
        .tag_version(version.version.id, "my_tag".to_string())
        .unwrap();
    assert_eq!(tagged.tag.unwrap(), "my_tag");
}

#[test]
fn test_dangling_parent_id_errors_instead_of_silent_none() {
    // Regression: resolving a version's parent UUID must distinguish a
    // genuinely absent parent_id (a real root version, Ok(None)) from a
    // parent_id that points at a row which no longer exists (corrupt/
    // ambiguous data, must be an error). Collapsing both into `None` would
    // make a dangling reference silently look like a root — exactly the bug
    // that would make a future sync's fast-forward/divergence check
    // misclassify a broken chain as a fresh one.
    let pool = init_db();
    let games_repo = SqliteGameRepository::new(pool.clone());
    let versions_repo = SqliteVersionRepository::new(pool.clone());

    let form = GameForm {
        id: None,
        uuid: None,
        title: "Dangling Parent Game",
        title_normalized: None,
        folder_slug: None,
        active_slot_id: None,
        icon_path: None,
    };
    let game = games_repo.save_game(form, None).unwrap();

    let parent = versions_repo
        .create_version_from_snapshot(game.id, VersionReason::Manual, empty_snapshot())
        .unwrap();
    let child = versions_repo
        .create_version_from_snapshot(game.id, VersionReason::Manual, empty_snapshot())
        .unwrap();
    assert_eq!(
        child.version.parent_id,
        Some(parent.version.id),
        "sanity check: the second version's parent is the first"
    );

    // Manufacture the dangling state: FK enforcement (parent_id REFERENCES
    // versions(id)) prevents this from arising naturally, so disable it just
    // for this delete, then remove the parent row directly, leaving the
    // child's parent_id pointing nowhere.
    {
        use diesel::prelude::*;
        use savegame_timeline_core::schema::versions::dsl as versions_dsl;
        let conn = &mut pool.get().unwrap();
        diesel::sql_query("PRAGMA foreign_keys = OFF")
            .execute(conn)
            .unwrap();
        diesel::delete(versions_dsl::versions.filter(versions_dsl::id.eq(parent.version.id)))
            .execute(conn)
            .unwrap();
    }

    let result = versions_repo.get_version(child.version.id);
    assert!(
        result.is_err(),
        "a parent_id pointing at a deleted row must error, not silently resolve to None"
    );
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

    // Manual is an arbitrary placeholder here too — no facade operation is
    // involved, so the specific reason carries no meaning beyond round-tripping.
    let version = versions_repo
        .create_version_from_snapshot(game.id, VersionReason::Manual, empty_snapshot())
        .unwrap();
    let version_dir =
        repository::version_dir(&slot_dir, version.version.folder_slug.as_deref().unwrap());
    let version_metadata: repository::VersionMetadata =
        repository::read_metadata_json(&version_dir).unwrap();
    assert_eq!(version_metadata.version_number, 1);
    assert_eq!(version_metadata.game_uuid, game_metadata.uuid);
    assert_eq!(version_metadata.reason, VersionReason::Manual);
}

#[test]
fn snapshot_metadata_failure_rolls_back_version_row_and_slot_pointer() {
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
                title: "Rollback Test",
                title_normalized: None,
                folder_slug: None,
                active_slot_id: None,
                icon_path: None,
            },
            None,
        )
        .unwrap();
    let full_game = games_repo.get_game(game.id).unwrap();
    let slot = full_game.active_slot.as_ref().unwrap();
    let game_dir = repository::game_dir(
        repository_root.path(),
        full_game.game.folder_slug.as_deref().unwrap(),
    );
    let slot_dir = repository::slot_dir(&game_dir, &slot.folder_slug);
    let versions_path = repository::versions_dir(&slot_dir);
    std::fs::write(&versions_path, b"not a directory").unwrap();

    let result = versions_repo.create_version_from_snapshot(
        game.id,
        VersionReason::Manual,
        CapturedSnapshot {
            content_hash: "sha256:0000000000000000000000000000000000000000000000000000000000000000"
                .to_string(),
            files: Vec::new(),
        },
    );

    assert!(result.is_err());
    let unchanged = games_repo.get_game(game.id).unwrap();
    assert!(unchanged.versions.is_empty());
    assert_eq!(
        unchanged
            .active_slot
            .as_ref()
            .and_then(|active_slot| active_slot.active_version_id),
        None
    );
}

#[test]
fn later_metadata_sync_failure_rolls_back_sqlite_and_removes_published_version_metadata() {
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
                title: "Late Failure Test",
                title_normalized: None,
                folder_slug: None,
                active_slot_id: None,
                icon_path: None,
            },
            None,
        )
        .unwrap();
    let full_game = games_repo.get_game(game.id).unwrap();
    let slot = full_game.active_slot.as_ref().unwrap();
    let game_dir = repository::game_dir(
        repository_root.path(),
        full_game.game.folder_slug.as_deref().unwrap(),
    );
    let slot_dir = repository::slot_dir(&game_dir, &slot.folder_slug);
    let versions_path = repository::versions_dir(&slot_dir);

    // Let version publication succeed, then make the later game metadata sync
    // fail at the slot metadata write.
    let slot_metadata_path = repository::metadata_path(&slot_dir);
    std::fs::remove_file(&slot_metadata_path).unwrap();
    std::fs::create_dir(&slot_metadata_path).unwrap();

    let result = versions_repo.create_version_from_snapshot(
        game.id,
        VersionReason::Manual,
        empty_snapshot(),
    );

    assert!(result.is_err());
    let unchanged = games_repo.get_game(game.id).unwrap();
    assert!(unchanged.versions.is_empty());
    assert_eq!(
        unchanged
            .active_slot
            .as_ref()
            .and_then(|active_slot| active_slot.active_version_id),
        None
    );
    assert!(versions_path.exists());
    assert_eq!(
        std::fs::read_dir(&versions_path)
            .unwrap()
            .filter_map(Result::ok)
            .count(),
        0,
        "a rolled-back transaction must not leave a published version directory"
    );
}

#[test]
fn snapshot_metadata_survives_tagging_and_activation() {
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
                title: "Preservation Test",
                title_normalized: None,
                folder_slug: None,
                active_slot_id: None,
                icon_path: None,
            },
            None,
        )
        .unwrap();
    let tracked_dir_uuid = uuid::Uuid::parse_str("44444444-4444-4444-4444-444444444444").unwrap();
    let snapshot = CapturedSnapshot {
        content_hash: "sha256:1111111111111111111111111111111111111111111111111111111111111111"
            .to_string(),
        files: vec![repository::VersionFileMetadata {
            tracked_dir_uuid,
            relative_path: "save.dat".to_string(),
            size: 9,
            sha256: "sha256:2222222222222222222222222222222222222222222222222222222222222222"
                .to_string(),
        }],
    };

    let version = versions_repo
        .create_version_from_snapshot(game.id, VersionReason::Manual, snapshot.clone())
        .unwrap();
    let game_dir = repository::game_dir(
        repository_root.path(),
        version.game.folder_slug.as_deref().unwrap(),
    );
    let slot_dir = repository::slot_dir(&game_dir, &version.slot.folder_slug);
    let version_dir =
        repository::version_dir(&slot_dir, version.version.folder_slug.as_deref().unwrap());
    let before: repository::VersionMetadata = repository::read_metadata_json(&version_dir).unwrap();
    assert_eq!(before.content_hash, snapshot.content_hash);
    assert_eq!(before.files, snapshot.files);

    let tagged = versions_repo
        .tag_version(version.version.id, "checkpoint".to_string())
        .unwrap();
    assert_eq!(tagged.tag.as_deref(), Some("checkpoint"));
    let after_tag: repository::VersionMetadata =
        repository::read_metadata_json(&version_dir).unwrap();
    assert_eq!(after_tag.content_hash, before.content_hash);
    assert_eq!(after_tag.files, before.files);
    assert_eq!(after_tag.tag.as_deref(), Some("checkpoint"));

    versions_repo
        .set_active_version(version.version.id)
        .unwrap();
    let after_activation: repository::VersionMetadata =
        repository::read_metadata_json(&version_dir).unwrap();
    assert_eq!(after_activation.content_hash, before.content_hash);
    assert_eq!(after_activation.files, before.files);
    assert_eq!(after_activation.tag.as_deref(), Some("checkpoint"));
}

#[test]
fn tagging_missing_version_metadata_fails_without_fabricating_a_manifest() {
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
                title: "Missing Metadata Tag Test",
                title_normalized: None,
                folder_slug: None,
                active_slot_id: None,
                icon_path: None,
            },
            None,
        )
        .unwrap();
    let version = versions_repo
        .create_version_from_snapshot(
            game.id,
            VersionReason::Manual,
            CapturedSnapshot {
                content_hash:
                    "sha256:1111111111111111111111111111111111111111111111111111111111111111"
                        .to_string(),
                files: Vec::new(),
            },
        )
        .unwrap();
    let game_dir = repository::game_dir(
        repository_root.path(),
        version.game.folder_slug.as_deref().unwrap(),
    );
    let slot_dir = repository::slot_dir(&game_dir, &version.slot.folder_slug);
    let version_dir =
        repository::version_dir(&slot_dir, version.version.folder_slug.as_deref().unwrap());
    std::fs::remove_dir_all(&version_dir).unwrap();

    assert!(
        versions_repo
            .tag_version(version.version.id, "checkpoint".to_string())
            .is_err()
    );
    assert!(!version_dir.exists());
    assert_eq!(
        versions_repo
            .get_version(version.version.id)
            .unwrap()
            .version
            .tag,
        None
    );
}

#[test]
fn activation_does_not_recreate_missing_version_metadata() {
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
                title: "Missing Metadata Activation Test",
                title_normalized: None,
                folder_slug: None,
                active_slot_id: None,
                icon_path: None,
            },
            None,
        )
        .unwrap();
    let version = versions_repo
        .create_version_from_snapshot(
            game.id,
            VersionReason::Manual,
            CapturedSnapshot {
                content_hash:
                    "sha256:2222222222222222222222222222222222222222222222222222222222222222"
                        .to_string(),
                files: Vec::new(),
            },
        )
        .unwrap();
    let game_dir = repository::game_dir(
        repository_root.path(),
        version.game.folder_slug.as_deref().unwrap(),
    );
    let slot_dir = repository::slot_dir(&game_dir, &version.slot.folder_slug);
    let version_dir =
        repository::version_dir(&slot_dir, version.version.folder_slug.as_deref().unwrap());
    std::fs::remove_dir_all(&version_dir).unwrap();

    versions_repo
        .set_active_version(version.version.id)
        .unwrap();
    assert!(!version_dir.exists());
}

fn seed_game_with_slot(conn: &mut SqliteConnection, title: &str) -> (i32, i32) {
    use diesel::prelude::*;
    use savegame_timeline_core::schema::{games, slots};

    let game_id: i32 = diesel::insert_into(games::table)
        .values((
            games::title.eq(title),
            games::title_normalized.eq(title.to_lowercase()),
        ))
        .returning(games::id)
        .get_result(conn)
        .unwrap();

    let slot_id: i32 = diesel::insert_into(slots::table)
        .values((
            slots::uuid.eq("aaaaaaaa-0000-0000-0000-000000000000"),
            slots::game_id.eq(game_id),
            slots::name.eq("Default Slot"),
            slots::name_normalized.eq("default slot"),
            slots::folder_slug.eq("default-slot__s_aaaaaaaa"),
        ))
        .returning(slots::id)
        .get_result(conn)
        .unwrap();

    (game_id, slot_id)
}

#[test]
fn test_duplicate_version_numbers_allowed_within_a_slot() {
    use diesel::prelude::*;
    use savegame_timeline_core::schema::versions;

    let pool = init_db();
    let conn = &mut pool.get().unwrap();

    let (_game_id, slot_id) = seed_game_with_slot(conn, "Dup Number Game");

    for uuid in [
        "11111111-1111-1111-1111-111111111111",
        "22222222-2222-2222-2222-222222222222",
    ] {
        diesel::insert_into(versions::table)
            .values((
                versions::uuid.eq(uuid),
                versions::folder_slug.eq(format!("v005__v_{}", &uuid[..8])),
                versions::version_number.eq(5),
                versions::slot_id.eq(slot_id),
                versions::reason.eq("watcher"),
            ))
            .execute(conn)
            .expect("both versions numbered 5 must insert");
    }

    let count: i64 = versions::table
        .filter(versions::slot_id.eq(slot_id))
        .filter(versions::version_number.eq(5))
        .count()
        .get_result(conn)
        .unwrap();

    assert_eq!(
        count, 2,
        "two versions may share a version_number in one slot"
    );
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
