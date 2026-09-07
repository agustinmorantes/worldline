use core::time::Duration;
use diesel::SqliteConnection;
use diesel::r2d2::{ConnectionManager, Pool};
use diesel_migrations::MigrationHarness;
use savegame_timeline_core::MIGRATIONS;
use savegame_timeline_core::TimelineCore;
use savegame_timeline_core::checkpoint::{self, CheckpointOutcome};
use savegame_timeline_core::models::*;
use savegame_timeline_core::persistence::sqlite::{
    SqliteGameRepository, SqliteScanRootsRepository, SqliteVersionRepository,
};
use savegame_timeline_core::persistence::{GameRepository, VersionRepository};
use savegame_timeline_core::repository::VersionReason;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::tempdir;
use uuid::Uuid;

#[cfg(unix)]
fn create_directory_link(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[cfg(windows)]
fn create_directory_link(target: &Path, link: &Path) {
    let output = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "failed to create test junction: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn unchanged_checkpoint_reuses_active_version_and_changed_checkpoint_captures_every_directory() {
    let mut harness = TwoDirHarness::new();
    let initial = harness.active_version();

    let CheckpointOutcome::Unchanged(unchanged) = harness.checkpoint(VersionReason::Manual) else {
        panic!("identical content must return an unchanged checkpoint outcome");
    };
    assert_eq!(unchanged.version.id, initial.version.id);
    assert_eq!(
        harness
            .core
            .get_game(harness.game.game.id)
            .unwrap()
            .versions
            .len(),
        1
    );

    fs::write(harness.first_dir.join("changed.dat"), b"changed").unwrap();
    let CheckpointOutcome::Created(created) = harness.checkpoint(VersionReason::Manual) else {
        panic!("changed content must return a created checkpoint outcome");
    };
    let metadata = harness.read_version_metadata(&created);
    assert!(
        metadata
            .files
            .iter()
            .any(|file| file.tracked_dir_uuid == harness.first_uuid)
    );
    assert!(
        metadata
            .files
            .iter()
            .any(|file| file.tracked_dir_uuid == harness.second_uuid)
    );
}

#[test]
fn restore_resolves_retired_tracked_directories() {
    let mut harness = TwoDirHarness::new();
    let initial = harness.active_version();
    let form = |id| GameForm {
        id: Some(id),
        uuid: None,
        title: "Two Directory Test",
        title_normalized: None,
        folder_slug: None,
        active_slot_id: None,
        icon_path: None,
    };
    harness
        .core
        .save_game(
            form(harness.game.game.id),
            Some(vec![harness.second_dir.to_str().unwrap()]),
        )
        .unwrap();
    fs::write(harness.first_dir.join("first.dat"), b"retired changes").unwrap();
    fs::write(harness.second_dir.join("second.dat"), b"live changes").unwrap();
    harness.core.restore_version(initial.version.id).unwrap();
    assert_eq!(
        fs::read(harness.first_dir.join("first.dat")).unwrap(),
        b"first"
    );
    assert_eq!(
        fs::read(harness.second_dir.join("second.dat")).unwrap(),
        b"second"
    );
    harness
        .core
        .save_game(
            form(harness.game.game.id),
            Some(vec![
                harness.first_dir.to_str().unwrap(),
                harness.second_dir.to_str().unwrap(),
            ]),
        )
        .unwrap();
    harness.game = harness.core.get_game(harness.game.game.id).unwrap();
    fs::write(harness.first_dir.join("new.dat"), b"new checkpoint").unwrap();
    let CheckpointOutcome::Created(captured) = harness.checkpoint(VersionReason::Manual) else {
        panic!("changed files must create a checkpoint");
    };
    let metadata = harness.read_version_metadata(&captured);
    assert!(
        metadata
            .files
            .iter()
            .any(|file| file.tracked_dir_uuid == harness.first_uuid
                && file.relative_path == "new.dat")
    );
    assert!(
        metadata
            .files
            .iter()
            .any(|file| file.tracked_dir_uuid == harness.second_uuid
                && file.relative_path == "second.dat")
    );
}

#[test]
fn restore_uses_manifest_as_the_complete_listing() {
    let harness = RestoreHarness::version_with_files(&[("keep.dat", b"old")]);
    fs::write(harness.live_dir.join("extra.dat"), b"must disappear").unwrap();
    harness.core.restore_version(harness.version.id).unwrap();
    assert_eq!(fs::read(harness.live_dir.join("keep.dat")).unwrap(), b"old");
    assert!(!harness.live_dir.join("extra.dat").exists());
}

#[test]
fn restore_replaces_file_with_snapshot_directory() {
    let harness = RestoreHarness::version_with_files(&[("a/save.dat", b"snapshotted")]);
    let nested = harness.live_dir.join("a");
    fs::create_dir(&nested).unwrap();
    fs::remove_dir(&nested).unwrap();
    fs::write(&nested, b"blocking file").unwrap();

    harness.core.restore_version(harness.version.id).unwrap();

    assert!(nested.is_dir());
    assert_eq!(fs::read(nested.join("save.dat")).unwrap(), b"snapshotted");
}

#[test]
fn restore_preserves_excluded_file_ancestor_and_restores_other_root() {
    let harness = RestoreHarness::version_with_directories(&[
        &[("a/save.dat", b"blocked snapshot")],
        &[("other/save.dat", b"other snapshot")],
    ]);
    let game_id = harness.core.get_games().unwrap().first().unwrap().game.id;

    {
        use diesel::prelude::*;
        use savegame_timeline_core::schema::tracked_dirs;
        let conn = &mut harness.pool.get().unwrap();
        diesel::update(
            tracked_dirs::table
                .filter(tracked_dirs::game_id.eq(game_id))
                .filter(tracked_dirs::path.eq(harness.live_dir.to_str().unwrap())),
        )
        .set(tracked_dirs::exclusions.eq(r#"["a"]"#))
        .execute(conn)
        .unwrap();
    }

    let blocked = harness.live_dir.join("a");
    fs::create_dir(&blocked).unwrap();
    fs::remove_dir(&blocked).unwrap();
    fs::write(&blocked, b"keep this file").unwrap();
    let other = harness._live_dirs[1].path().join("other/save.dat");
    fs::create_dir_all(other.parent().unwrap()).unwrap();
    fs::write(&other, b"changed other").unwrap();

    harness.core.restore_version(harness.version.id).unwrap();

    assert_eq!(fs::read(&blocked).unwrap(), b"keep this file");
    assert_eq!(fs::read(&other).unwrap(), b"other snapshot");
}

#[test]
fn corrupt_blob_aborts_before_live_files_are_cleared() {
    let harness = RestoreHarness::version_with_files(&[("save.dat", b"known good")]);
    fs::write(harness.blob_for("save.dat"), b"corrupt").unwrap();
    fs::write(harness.live_dir.join("save.dat"), b"current progress").unwrap();
    assert!(harness.core.restore_version(harness.version.id).is_err());
    assert_eq!(
        fs::read(harness.live_dir.join("save.dat")).unwrap(),
        b"current progress"
    );
}

#[test]
fn corrupt_target_leaves_active_version_and_live_files_unchanged() {
    let harness = RestoreHarness::version_with_files(&[("save.dat", b"known good")]);
    let before = harness
        .core
        .get_games()
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    let before_active_slot_id = before.game.active_slot_id;
    let before_active_version_id = before
        .active_slot
        .as_ref()
        .and_then(|slot| slot.active_version_id);
    fs::write(harness.blob_for("save.dat"), b"corrupt").unwrap();
    fs::write(harness.live_dir.join("save.dat"), b"current progress").unwrap();

    assert!(harness.core.restore_version(harness.version.id).is_err());

    let after = harness
        .core
        .get_games()
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(after.game.active_slot_id, before_active_slot_id);
    assert_eq!(
        after
            .active_slot
            .as_ref()
            .and_then(|slot| slot.active_version_id),
        before_active_version_id
    );
    assert_eq!(
        fs::read(harness.live_dir.join("save.dat")).unwrap(),
        b"current progress"
    );
}

#[test]
fn content_hash_mismatch_aborts_before_live_files_are_cleared() {
    let harness = RestoreHarness::version_with_files(&[("save.dat", b"known good")]);
    let metadata_path = harness.version_dir.join("metadata.json");
    let mut metadata: savegame_timeline_core::repository::VersionMetadata =
        savegame_timeline_core::repository::read_metadata_json(&harness.version_dir).unwrap();
    metadata.content_hash =
        "sha256:0000000000000000000000000000000000000000000000000000000000000000".to_string();
    fs::write(metadata_path, serde_json::to_vec_pretty(&metadata).unwrap()).unwrap();
    fs::write(harness.live_dir.join("save.dat"), b"current progress").unwrap();

    assert!(harness.core.restore_version(harness.version.id).is_err());
    assert_eq!(
        fs::read(harness.live_dir.join("save.dat")).unwrap(),
        b"current progress"
    );
}

#[test]
fn noncanonical_manifest_order_aborts_before_live_files_are_cleared() {
    let harness = RestoreHarness::version_with_files(&[("a.dat", b"a"), ("b.dat", b"b")]);
    harness.rewrite_manifest(|metadata| metadata.files.swap(0, 1));
    fs::write(harness.live_dir.join("a.dat"), b"current progress").unwrap();

    assert!(harness.core.restore_version(harness.version.id).is_err());
    assert_eq!(
        fs::read(harness.live_dir.join("a.dat")).unwrap(),
        b"current progress"
    );
}

#[cfg(any(unix, windows))]
#[test]
fn linked_restore_paths_abort_before_any_tracked_directory_is_mutated() {
    for link_is_live_root in [false, true] {
        let second_file = if link_is_live_root {
            "second.dat"
        } else {
            "nested/second.dat"
        };
        let harness = RestoreHarness::version_with_directories(&[
            &[("first.dat", b"old first")],
            &[(second_file, b"old second")],
        ]);
        let first_dir = &harness.live_dir;
        let second_dir = harness._live_dirs[1].path();
        let external = tempdir().unwrap();
        let external_file = external.path().join("second.dat");
        fs::write(&external_file, b"external progress").unwrap();
        fs::write(first_dir.join("first.dat"), b"current first").unwrap();
        fs::write(first_dir.join("extra.dat"), b"current extra").unwrap();

        if link_is_live_root {
            fs::remove_dir(second_dir).unwrap();
            create_directory_link(external.path(), second_dir);
        } else {
            create_directory_link(external.path(), &second_dir.join("nested"));
        }

        assert!(harness.core.restore_version(harness.version.id).is_err());
        assert_eq!(
            fs::read(first_dir.join("first.dat")).unwrap(),
            b"current first"
        );
        assert_eq!(
            fs::read(first_dir.join("extra.dat")).unwrap(),
            b"current extra"
        );
        assert_eq!(fs::read(&external_file).unwrap(), b"external progress");
    }
}

#[test]
fn unsafe_manifest_path_aborts_before_live_files_are_cleared() {
    let harness = RestoreHarness::version_with_files(&[("save.dat", b"known good")]);
    harness.rewrite_manifest(|metadata| {
        metadata.files[0].relative_path = "../outside.dat".to_string();
    });
    fs::write(harness.live_dir.join("save.dat"), b"current progress").unwrap();

    assert!(harness.core.restore_version(harness.version.id).is_err());
    assert_eq!(
        fs::read(harness.live_dir.join("save.dat")).unwrap(),
        b"current progress"
    );
}

#[test]
fn unknown_tracked_directory_aborts_before_live_files_are_cleared() {
    let harness = RestoreHarness::version_with_files(&[("save.dat", b"known good")]);
    harness.rewrite_manifest(|metadata| {
        metadata.files[0].tracked_dir_uuid = Uuid::new_v4();
    });
    fs::write(harness.live_dir.join("save.dat"), b"current progress").unwrap();

    assert!(harness.core.restore_version(harness.version.id).is_err());
    assert_eq!(
        fs::read(harness.live_dir.join("save.dat")).unwrap(),
        b"current progress"
    );
}

#[test]
fn missing_and_same_size_corrupt_blobs_abort_before_live_files_are_cleared() {
    for corrupt_blob in [false, true] {
        let harness = RestoreHarness::version_with_files(&[("save.dat", b"known good")]);
        let blob = harness.blob_for("save.dat");
        if corrupt_blob {
            fs::write(blob, b"bad bytes!").unwrap();
        } else {
            fs::remove_file(blob).unwrap();
        }
        fs::write(harness.live_dir.join("save.dat"), b"current progress").unwrap();

        assert!(harness.core.restore_version(harness.version.id).is_err());
        assert_eq!(
            fs::read(harness.live_dir.join("save.dat")).unwrap(),
            b"current progress"
        );
    }
}

#[test]
fn test_restore_uses_version_id_not_version_number() {
    // Regression: restore_version must identify the version by version.id (the DB
    // primary key), not by version_number (the per-slot sequential counter). For
    // Game 2's first version, version.id=2 but version_number=1. Its manifest is
    // loaded from the slug-based, slot-nested path derived from that version's
    // own identity, so confusing the two must not restore the wrong version.
    let pool = init_db();
    let data_dir = tempdir().unwrap();
    let working_base = data_dir.path().join("repository");
    let icons_dir = data_dir.path().join("icons");
    let games_repo = Arc::new(SqliteGameRepository::with_repository_root(
        pool.clone(),
        working_base.clone(),
    ));
    let versions_repo = Arc::new(SqliteVersionRepository::with_repository_root(
        pool.clone(),
        working_base.clone(),
    ));
    let scan_roots_repo = Arc::new(SqliteScanRootsRepository::new(pool.clone()));

    let game1_dir = tempdir().unwrap();
    let game2_dir = tempdir().unwrap();
    fs::write(game1_dir.path().join("save.dat"), "game1 data").unwrap();
    // game2 initial content is written before game2 is registered. Its first
    // authoritative manifest will be created only when the later change event
    // is observed; no legacy snapshot tree is expected during registration.
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

    // Trigger a version for Game 1 — this consumes version id=1, so Game 2's
    // first version will get id=2 (while its version_number will be 1).
    assert!(write_until_version_count(
        &core,
        &game1_dir.path().join("save.dat"),
        "game1 modified",
        game1.id,
        1,
        Duration::from_secs(30),
    ));
    assert!(wait_for_change_detected(
        &notif_rx,
        game1.id,
        Duration::from_secs(30),
    ));

    // Now write the state we want to restore to. After this, the watcher will
    // create version id=2 (version_number=1) for Game 2, snapshotting this content.
    assert!(write_until_version_count(
        &core,
        &game2_dir.path().join("save.dat"),
        "game2 good save",
        game2.id,
        1,
        Duration::from_secs(30),
    ));
    assert!(wait_for_change_detected(
        &notif_rx,
        game2.id,
        Duration::from_secs(30),
    ));

    let full_game2 = core.get_game(game2.id).unwrap();
    let version = full_game2
        .versions
        .last()
        .expect("Game 2 should have at least one version");

    // Precondition: this is the exact mismatch the bug exploited.
    assert_eq!(
        version.version_number, 1,
        "Game 2's first version_number should be 1"
    );
    assert!(
        version.id > version.version_number,
        "Game 2's version id ({}) should be greater than its version_number ({}) \
         because Game 1's version consumed id=1 first",
        version.id,
        version.version_number
    );

    // ChangeDetected is sent only after the full checkpoint pass completes, so
    // it is the synchronization point before corrupting and restoring.
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

    // Trigger first change and wait for version 1 to be created.
    assert!(write_until_version_count(
        &core,
        &save_file,
        "change 1",
        game.id,
        1,
        Duration::from_secs(30),
    ));
    assert!(wait_for_change_detected(
        &notif_rx,
        game.id,
        Duration::from_secs(30),
    ));

    // Trigger a second change immediately — still well within the 15-minute cooldown.
    assert!(write_until_change_detected(
        &notif_rx,
        &save_file,
        "change 2",
        game.id,
        Duration::from_secs(30),
    ));

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

    let mut version_created = false;
    let mut change_detected = false;

    // Registration is asynchronous. Keep nudging the file until the watcher
    // observes it, rather than racing a fixed sleep on clean builds where the
    // watcher thread may not have processed ReregisterGames yet.
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while std::time::Instant::now() < deadline && !(version_created && change_detected) {
        if !version_created {
            fs::write(&save_file, "modified text").unwrap();
        }
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
        if !(version_created && change_detected) {
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    assert!(version_created, "Version was not created automatically");
    assert!(change_detected, "Change was not detected automatically");

    let full_game = core.get_game(game.id).unwrap();
    let version = full_game.versions.last().unwrap();
    let game_dir = savegame_timeline_core::repository::game_dir(
        working_base.path(),
        full_game.game.folder_slug.as_deref().unwrap(),
    );
    let slot_dir = savegame_timeline_core::repository::slot_dir(
        &game_dir,
        &full_game.active_slot.as_ref().unwrap().folder_slug,
    );
    let version_dir = savegame_timeline_core::repository::version_dir(
        &slot_dir,
        version.folder_slug.as_deref().unwrap(),
    );
    let metadata: savegame_timeline_core::repository::VersionMetadata =
        savegame_timeline_core::repository::read_metadata_json(&version_dir).unwrap();
    let file = metadata
        .files
        .iter()
        .find(|file| file.relative_path == "save.dat")
        .unwrap();
    let blob = savegame_timeline_core::content_store::blob_path(working_base.path(), &file.sha256)
        .unwrap();
    let content = fs::read_to_string(blob).unwrap();
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
    let initial_second_version_id = game_with_new_slot.versions[0].id;
    assert_eq!(
        game_with_new_slot.active_slot.as_ref().unwrap().name,
        "Second Slot"
    );
    assert_eq!(fs::read_dir(source_dir.path()).unwrap().count(), 0);

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

    let second_slot_after_switch = core
        .get_slots(game.id)
        .unwrap()
        .into_iter()
        .find(|slot| slot.slot.id == second_slot_id)
        .unwrap();
    assert_eq!(
        second_slot_after_switch.versions.len(),
        1,
        "switching away from an unchanged empty slot must retain its initial baseline"
    );
    assert_eq!(
        second_slot_after_switch.versions[0].reason, "initial",
        "the retained version must be the slot's initial baseline"
    );
    assert_eq!(
        second_slot_after_switch.versions[0].id, initial_second_version_id,
        "switching away must retain the exact initial baseline row"
    );

    let second_game = core.switch_slot(game.id, second_slot_id).unwrap();
    assert_eq!(
        fs::read_dir(source_dir.path()).unwrap().count(),
        0,
        "the unchanged second slot must restore its empty baseline"
    );
    fs::write(&save_file, "slot-two-state").unwrap();
    assert_eq!(
        second_game.active_slot.as_ref().unwrap().name,
        "Second Slot"
    );

    core.switch_slot(game.id, default_slot_id).unwrap();
    assert_eq!(fs::read_to_string(&save_file).unwrap(), "slot-one-state");
    core.switch_slot(game.id, second_slot_id).unwrap();
    assert_eq!(fs::read_to_string(&save_file).unwrap(), "slot-two-state");
}

#[test]
fn test_switch_slot_restores_slot_active_version_not_newest() {
    let h = harness("v1-state");
    let game = h.create_game("Rollback Slot Test");
    let default_slot_id = h.core.get_game(game.id).unwrap().active_slot.unwrap().id;

    // Build two content-distinct versions in the default slot via the normal
    // checkpoint flow: leaving a slot always snapshots its current live state.
    fs::write(&h.save_file, "v2-state").unwrap();
    let with_second = h.core.create_slot(game.id, "Scratch").unwrap();
    let scratch_slot_id = with_second.active_slot.as_ref().unwrap().id;
    // ^ leaving default just now checkpointed "v2-state" as default's first version.

    h.core.switch_slot(game.id, default_slot_id).unwrap(); // restores it; disk = "v2-state"
    fs::write(&h.save_file, "v3-state").unwrap();
    h.core.switch_slot(game.id, scratch_slot_id).unwrap();
    // ^ leaving default again checkpoints "v3-state" as a second, NEWER version
    // and makes it the slot's active version too.

    let default_slot = h
        .core
        .get_slots(game.id)
        .unwrap()
        .into_iter()
        .find(|s| s.slot.id == default_slot_id)
        .unwrap();
    assert_eq!(
        default_slot.versions.len(),
        2,
        "default slot should hold both checkpointed versions"
    );
    let oldest_id = default_slot
        .versions
        .iter()
        .min_by_key(|v| v.id)
        .unwrap()
        .id;
    let newest_id = default_slot
        .versions
        .iter()
        .max_by_key(|v| v.id)
        .unwrap()
        .id;
    assert_ne!(
        oldest_id, newest_id,
        "need two distinct versions to test with"
    );

    // Force the default slot's active version back to the older one directly
    // at the persistence layer. `restore_version` can't be used for this setup
    // step: it also flips the game's currently active slot as a side effect
    // (SqliteVersionRepository::set_active_version), and switch_slot's own
    // checkpoint-on-leave means any rollback made through the facade while a
    // slot is active gets superseded by a fresh checkpoint the moment you
    // leave that slot — so active_version_id and "the slot's newest version"
    // can never actually diverge across a real round trip through the facade.
    // Writing the pointer directly is the only way to put the default slot
    // into the state switch_slot's fix is meant to handle, so this test
    // exercises switch_slot's own selection logic rather than the
    // orchestration around it.
    {
        use diesel::prelude::*;
        use savegame_timeline_core::schema::slots::dsl as slots_dsl;
        let mut conn = h.pool.get().unwrap();
        diesel::update(slots_dsl::slots.filter(slots_dsl::id.eq(default_slot_id)))
            .set(slots_dsl::active_version_id.eq(oldest_id))
            .execute(&mut conn)
            .unwrap();
    }

    // Currently active is Scratch. Switching into Default must restore the
    // OLDER version (the slot's active_version_id) — not "v3-state", which is
    // merely the slot's most recently created version.
    h.core.switch_slot(game.id, default_slot_id).unwrap();

    assert_eq!(
        fs::read_to_string(&h.save_file).unwrap(),
        "v2-state",
        "switching into the slot must restore its ACTIVE version, not its newest"
    );
}

#[test]
fn test_switch_slot_to_never_versioned_slot_clears_live_dir() {
    // A freshly created slot has active_version_id = NULL until something is
    // checkpointed into it. Switching to such a slot must clear the live
    // directory rather than leaving the previous slot's files in place under
    // the new slot's identity.
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
    fs::write(&save_file, "default-slot-state").unwrap();

    let (core, _notif_rx) = TimelineCore::new(
        games_repo.clone(),
        versions_repo,
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
                title: "Never Versioned Slot Test",
                title_normalized: None,
                folder_slug: None,
                active_slot_id: None,
                icon_path: None,
            },
            Some(vec![source_dir.path().to_str().unwrap()]),
        )
        .unwrap();

    // Create the slot at the repository layer directly, bypassing the
    // facade's `create_slot` (which immediately gives new slots a baseline
    // version). This slot never gets a version, so active_version_id stays NULL.
    let empty_slot = games_repo.create_slot(game.id, "Empty").unwrap();
    assert!(
        empty_slot.slot.active_version_id.is_none(),
        "precondition: slot must have no active version"
    );

    let after_switch = core.switch_slot(game.id, empty_slot.slot.id).unwrap();
    assert_eq!(
        after_switch.active_slot.as_ref().unwrap().id,
        empty_slot.slot.id
    );

    assert!(
        !save_file.exists(),
        "switching to a slot with no active version must clear the live directory, \
         not leave the previous slot's files behind"
    );
}

#[test]
fn test_version_metadata_records_parent_uuid() {
    // NOTE: the brief's version of this test drove `create_slot` once and
    // then inspected `get_game(...).versions`, which only ever returns the
    // *active* slot's versions. A freshly created scratch slot's baseline is
    // parentless by construction, so that setup can never produce a version
    // with a parent — it would panic on `.expect(...)` before ever reaching
    // the metadata assertion. Instead, build a genuine two-version chain
    // inside the SAME (default) slot: leaving a slot always checkpoints its
    // current live state as a new version whose parent is that slot's
    // previous active version, exactly as in
    // `test_switch_slot_restores_slot_active_version_not_newest` above.
    let h = harness("v1-state");
    let game = h.create_game("Parent Uuid Test");
    let default_slot_id = h.core.get_game(game.id).unwrap().active_slot.unwrap().id;

    fs::write(&h.save_file, "v2-state").unwrap();
    let with_second = h.core.create_slot(game.id, "Scratch").unwrap();
    let scratch_slot_id = with_second.active_slot.as_ref().unwrap().id;
    // ^ leaving default just now checkpointed "v2-state" as default's first
    // (root, parentless) version.

    h.core.switch_slot(game.id, default_slot_id).unwrap();
    fs::write(&h.save_file, "v3-state").unwrap();
    h.core.switch_slot(game.id, scratch_slot_id).unwrap();
    // ^ leaving default again checkpoints "v3-state" as a second version,
    // genuinely descending from the first.

    let default_slot = h
        .core
        .get_slots(game.id)
        .unwrap()
        .into_iter()
        .find(|s| s.slot.id == default_slot_id)
        .unwrap();
    assert_eq!(
        default_slot.versions.len(),
        2,
        "default slot should hold both checkpointed versions"
    );

    let root = default_slot
        .versions
        .iter()
        .find(|v| v.parent_id.is_none())
        .expect("first checkpoint should have no parent");
    let child = default_slot
        .versions
        .iter()
        .find(|v| v.parent_id.is_some())
        .expect("second checkpoint should have a parent");
    assert_eq!(
        child.parent_id.unwrap(),
        root.id,
        "second version must descend from the first"
    );

    let game_dir = savegame_timeline_core::repository::game_dir(
        &h.repo_root(),
        default_slot.game.folder_slug.as_deref().unwrap(),
    );
    let slot_dir =
        savegame_timeline_core::repository::slot_dir(&game_dir, &default_slot.slot.folder_slug);

    let child_dir = savegame_timeline_core::repository::version_dir(
        &slot_dir,
        child.folder_slug.as_deref().unwrap(),
    );
    let child_raw = fs::read_to_string(child_dir.join("metadata.json")).unwrap();
    let child_parsed: serde_json::Value = serde_json::from_str(&child_raw).unwrap();
    assert_eq!(
        child_parsed["parent_version_uuid"].as_str().unwrap(),
        root.uuid.as_deref().unwrap(),
        "version metadata must record its real parent UUID"
    );

    let root_dir = savegame_timeline_core::repository::version_dir(
        &slot_dir,
        root.folder_slug.as_deref().unwrap(),
    );
    let root_raw = fs::read_to_string(root_dir.join("metadata.json")).unwrap();
    let root_parsed: serde_json::Value = serde_json::from_str(&root_raw).unwrap();
    assert!(
        root_parsed["parent_version_uuid"].is_null(),
        "a root version (no parent) must still write a null parent_version_uuid"
    );
}

/// Everything a facade test needs. Fields are kept alive by the struct:
/// dropping a `TempDir` deletes it, so these must outlive the test body.
struct RestoreHarness {
    core: TimelineCore,
    pool: Pool<ConnectionManager<SqliteConnection>>,
    version: Version,
    live_dir: PathBuf,
    repository_root: PathBuf,
    version_dir: PathBuf,
    _data_dir: tempfile::TempDir,
    _live_dirs: Vec<tempfile::TempDir>,
    _icons_dir: tempfile::TempDir,
    _notif_rx: savegame_timeline_core::detection::NotificationReceiver,
}

impl RestoreHarness {
    fn version_with_files(files: &[(&str, &[u8])]) -> Self {
        Self::version_with_directories(&[files])
    }

    fn version_with_directories(directories: &[&[(&str, &[u8])]]) -> Self {
        let pool = init_db();
        let data_dir = tempdir().unwrap();
        let repository_root = data_dir.path().join("repository");
        let live_dirs = directories
            .iter()
            .map(|files| {
                let live_dir = tempdir().unwrap();
                for (relative_path, contents) in *files {
                    let path = live_dir.path().join(relative_path);
                    fs::create_dir_all(path.parent().unwrap()).unwrap();
                    fs::write(path, contents).unwrap();
                }
                live_dir
            })
            .collect::<Vec<_>>();
        let icons_dir = tempdir().unwrap();

        let games_repo = Arc::new(SqliteGameRepository::with_repository_root(
            pool.clone(),
            repository_root.clone(),
        ));
        let versions_repo = Arc::new(SqliteVersionRepository::with_repository_root(
            pool.clone(),
            repository_root.clone(),
        ));
        let scan_roots_repo = Arc::new(SqliteScanRootsRepository::new(pool.clone()));
        let (core, notif_rx) = TimelineCore::new(
            games_repo,
            versions_repo,
            scan_roots_repo,
            repository_root.clone(),
            icons_dir.path().to_path_buf(),
            Some(0.1),
        )
        .unwrap();
        let game = core
            .save_game(
                GameForm {
                    id: None,
                    uuid: None,
                    title: "Restore Test",
                    title_normalized: None,
                    folder_slug: None,
                    active_slot_id: None,
                    icon_path: None,
                },
                Some(
                    live_dirs
                        .iter()
                        .map(|live_dir| live_dir.path().to_str().unwrap())
                        .collect(),
                ),
            )
            .unwrap();
        let default_slot_id = core.get_game(game.id).unwrap().active_slot.unwrap().id;
        core.create_slot(game.id, "Scratch").unwrap();
        let default_slot = core
            .get_slots(game.id)
            .unwrap()
            .into_iter()
            .find(|slot| slot.slot.id == default_slot_id)
            .unwrap();
        let version = default_slot
            .versions
            .iter()
            .find(|version| version.reason == "pre_slot_create")
            .unwrap()
            .clone();
        let version_dir = savegame_timeline_core::repository::version_dir(
            &savegame_timeline_core::repository::slot_dir(
                &savegame_timeline_core::repository::game_dir(
                    &repository_root,
                    default_slot.game.folder_slug.as_deref().unwrap(),
                ),
                &default_slot.slot.folder_slug,
            ),
            version.folder_slug.as_deref().unwrap(),
        );

        Self {
            core,
            pool,
            version,
            live_dir: live_dirs[0].path().to_path_buf(),
            repository_root,
            version_dir,
            _data_dir: data_dir,
            _live_dirs: live_dirs,
            _icons_dir: icons_dir,
            _notif_rx: notif_rx,
        }
    }

    fn blob_for(&self, relative_path: &str) -> PathBuf {
        let metadata: savegame_timeline_core::repository::VersionMetadata =
            savegame_timeline_core::repository::read_metadata_json(&self.version_dir).unwrap();
        let file = metadata
            .files
            .iter()
            .find(|file| file.relative_path == relative_path)
            .unwrap();
        savegame_timeline_core::content_store::blob_path(&self.repository_root, &file.sha256)
            .unwrap()
    }

    fn rewrite_manifest(
        &self,
        mutate: impl FnOnce(&mut savegame_timeline_core::repository::VersionMetadata),
    ) {
        let mut metadata: savegame_timeline_core::repository::VersionMetadata =
            savegame_timeline_core::repository::read_metadata_json(&self.version_dir).unwrap();
        mutate(&mut metadata);
        let manifest = serde_json::to_vec(&metadata.files).unwrap();
        metadata.content_hash = format!("sha256:{:x}", Sha256::digest(manifest));
        fs::write(
            self.version_dir.join("metadata.json"),
            serde_json::to_vec_pretty(&metadata).unwrap(),
        )
        .unwrap();
    }
}

impl Drop for RestoreHarness {
    fn drop(&mut self) {
        self.core.shutdown();
    }
}

#[allow(dead_code)]
struct TwoDirHarness {
    core: TimelineCore,
    versions_repo: Arc<SqliteVersionRepository>,
    game: FullGame,
    first_dir: PathBuf,
    second_dir: PathBuf,
    first_uuid: Uuid,
    second_uuid: Uuid,
    repository_root: PathBuf,
    _data_dir: tempfile::TempDir,
    _first_dir: tempfile::TempDir,
    _second_dir: tempfile::TempDir,
    _icons_dir: tempfile::TempDir,
    _notif_rx: savegame_timeline_core::detection::NotificationReceiver,
}
impl TwoDirHarness {
    fn new() -> Self {
        let pool = init_db();
        let data_dir = tempdir().unwrap();
        let repository_root = data_dir.path().join("repository");
        let first = tempdir().unwrap();
        let second = tempdir().unwrap();
        let icons_dir = tempdir().unwrap();
        fs::write(first.path().join("first.dat"), b"first").unwrap();
        fs::write(second.path().join("second.dat"), b"second").unwrap();

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
            Some(0.1),
        )
        .unwrap();

        let game = core
            .save_game(
                GameForm {
                    id: None,
                    uuid: None,
                    title: "Two Directory Test",
                    title_normalized: None,
                    folder_slug: None,
                    active_slot_id: None,
                    icon_path: None,
                },
                Some(vec![
                    first.path().to_str().unwrap(),
                    second.path().to_str().unwrap(),
                ]),
            )
            .unwrap();
        let game = core.get_game(game.id).unwrap();
        let tracked_dirs = game.tracked_dirs.clone();
        assert_eq!(tracked_dirs.len(), 2);
        let first_uuid = Uuid::parse_str(tracked_dirs[0].uuid.as_deref().unwrap()).unwrap();
        let second_uuid = Uuid::parse_str(tracked_dirs[1].uuid.as_deref().unwrap()).unwrap();
        let initial = checkpoint::checkpoint_current_state(
            &repository_root,
            versions_repo.as_ref(),
            &game,
            VersionReason::Initial,
        )
        .unwrap();
        let CheckpointOutcome::Created(_) = initial else {
            panic!("the harness must create an initial version");
        };
        let refreshed_game = core.get_game(game.game.id).unwrap();

        Self {
            core,
            versions_repo,
            game: refreshed_game,
            first_dir: first.path().to_path_buf(),
            second_dir: second.path().to_path_buf(),
            first_uuid,
            second_uuid,
            repository_root,
            _data_dir: data_dir,
            _first_dir: first,
            _second_dir: second,
            _icons_dir: icons_dir,
            _notif_rx: notif_rx,
        }
    }

    fn active_version(&self) -> FullVersion {
        self.versions_repo
            .get_version(self.game.last_version.as_ref().unwrap().id)
            .unwrap()
    }

    fn checkpoint(&mut self, reason: VersionReason) -> CheckpointOutcome {
        let outcome = checkpoint::checkpoint_current_state(
            &self.repository_root,
            self.versions_repo.as_ref(),
            &self.game,
            reason,
        )
        .unwrap();
        self.game = self.core.get_game(self.game.game.id).unwrap();
        outcome
    }

    fn read_version_metadata(
        &self,
        version: &FullVersion,
    ) -> savegame_timeline_core::repository::VersionMetadata {
        let game_dir = savegame_timeline_core::repository::game_dir(
            &self.repository_root,
            version.game.folder_slug.as_deref().unwrap(),
        );
        let slot_dir =
            savegame_timeline_core::repository::slot_dir(&game_dir, &version.slot.folder_slug);
        let version_dir = savegame_timeline_core::repository::version_dir(
            &slot_dir,
            version.version.folder_slug.as_deref().unwrap(),
        );
        savegame_timeline_core::repository::read_metadata_json(&version_dir).unwrap()
    }
}

impl Drop for TwoDirHarness {
    fn drop(&mut self) {
        self.core.shutdown();
    }
}

#[allow(dead_code)]
struct Harness {
    core: TimelineCore,
    pool: Pool<ConnectionManager<SqliteConnection>>,
    working_base: tempfile::TempDir,
    source_dir: tempfile::TempDir,
    _icons_dir: tempfile::TempDir,
    save_file: std::path::PathBuf,
    /// The watcher thread shuts itself down as soon as a notification send
    /// fails, so this receiver has to outlive everything the test does.
    _notif_rx: savegame_timeline_core::detection::NotificationReceiver,
}

fn harness(initial_contents: &str) -> Harness {
    let pool = init_db();
    let working_base = tempdir().unwrap();
    // Nested one level on purpose: TimelineCore derives the monitoring settings
    // path from working_base.parent(), so a bare tempdir would resolve it to the
    // shared system temp directory and every harness test would read - and one
    // day write - the same file.
    let repo_root = working_base.path().join("repository");
    let source_dir = tempdir().unwrap();
    let icons_dir = tempdir().unwrap();

    let save_file = source_dir.path().join("save.dat");
    fs::write(&save_file, initial_contents).unwrap();

    let games_repo = Arc::new(SqliteGameRepository::with_repository_root(
        pool.clone(),
        repo_root.clone(),
    ));
    let versions_repo = Arc::new(SqliteVersionRepository::with_repository_root(
        pool.clone(),
        repo_root.clone(),
    ));
    let scan_roots_repo = Arc::new(SqliteScanRootsRepository::new(pool.clone()));

    let (core, notif_rx) = TimelineCore::new(
        games_repo,
        versions_repo,
        scan_roots_repo,
        repo_root.clone(),
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
        _notif_rx: notif_rx,
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        // Every harness leaves a watcher thread (and its debouncer) running for
        // the whole test binary otherwise. Fourteen of those ticking at once
        // steal exactly the timing headroom the poll loops in this file depend on.
        self.core.shutdown();
    }
}

impl Harness {
    /// The repository root the core writes into — one level inside `working_base`.
    fn repo_root(&self) -> std::path::PathBuf {
        self.working_base.path().join("repository")
    }

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

    /// Pushes every version well outside the watcher's `version_cooldown_minutes`
    /// window (15 by default), so a later genuine change really does produce a
    /// new version instead of being silently swallowed by the cooldown.
    fn backdate_versions(&self) {
        use diesel::prelude::*;
        use savegame_timeline_core::schema::versions;
        let long_ago = chrono::Utc::now().naive_utc() - chrono::Duration::hours(1);
        let conn = &mut self.pool.get().unwrap();
        diesel::update(versions::table)
            .set(versions::created_at.eq(long_ago))
            .execute(conn)
            .unwrap();
    }
}

#[test]
fn test_version_reason_is_persisted_per_operation() {
    // NOTE: the brief's version of this test asserted that the very first
    // version returned by `h.create_game(...)` alone has reason "initial".
    // That doesn't hold in this codebase: `save_game`/`ensure_default_slot`
    // creates the default slot but never calls `create_version` — the first
    // version only appears once something actually checkpoints (create_slot's
    // baseline) or the watcher fires. So this test drives `create_slot` and
    // `switch_slot` instead, which are the real call sites that tag versions
    // with distinct reasons, and checks all three resulting reasons are
    // present and different — not just "some string is set" everywhere.
    let h = harness("initial-state");
    let game = h.create_game("Reason Test");
    let default_slot_id = h.core.get_game(game.id).unwrap().active_slot.unwrap().id;

    // Creating a second slot checkpoints the outgoing (default) slot with
    // reason=pre_slot_create and gives the new slot an empty baseline with
    // reason=initial.
    fs::write(&h.save_file, "changed-before-slot-create").unwrap();
    let after_slot = h.core.create_slot(game.id, "Second").unwrap();
    let second_slot_id = after_slot.active_slot.as_ref().unwrap().id;

    let baseline = after_slot.versions.iter().find(|v| v.reason == "initial");
    assert!(
        baseline.is_some(),
        "the new slot's baseline version is created with reason=initial"
    );

    let slots = h.core.get_slots(game.id).unwrap();
    let default_slot = slots.iter().find(|s| s.slot.id == default_slot_id).unwrap();
    let pre_create = default_slot
        .versions
        .iter()
        .find(|v| v.reason == "pre_slot_create");
    assert!(
        pre_create.is_some(),
        "leaving the default slot to create a new one checkpoints it with reason=pre_slot_create"
    );

    // Switching back to the default slot checkpoints the outgoing (second)
    // slot with yet another, distinct reason: pre_switch.
    fs::write(&h.save_file, "changed-before-switch").unwrap();
    h.core.switch_slot(game.id, default_slot_id).unwrap();

    let slots_after_switch = h.core.get_slots(game.id).unwrap();
    let second_slot = slots_after_switch
        .iter()
        .find(|s| s.slot.id == second_slot_id)
        .unwrap();
    let pre_switch = second_slot
        .versions
        .iter()
        .find(|v| v.reason == "pre_switch");
    assert!(
        pre_switch.is_some(),
        "switching away from a slot checkpoints it with reason=pre_switch"
    );
}

#[test]
fn test_restore_version_rejects_orphaned_version_without_mutating_slot() {
    // NOTE: the brief's own repro (`restore_version(999_999)`) can't expose this
    // bug. A nonexistent version id fails at the very first `SELECT ... FROM
    // versions` lookup, and that lookup is identical in the buggy code (inside
    // `set_active_version`) and the fixed code (inside `get_version`) — so
    // nothing is ever mutated either way, and the test can't tell old from new
    // behavior. `game.slots.iter().any(|slot| slot.id == version.slot_id)` is
    // similarly untestable through the public API: `game` is always fetched via
    // `version.game.id`, which is itself derived from `version.slot_id`, so that
    // predicate is tautologically true for any consistent row graph reachable
    // through the facade.
    //
    // This SQLite build enforces foreign keys (confirmed empirically: calling
    // the facade's own `delete_game` on a game with slots/versions attached
    // fails with "FOREIGN KEY constraint failed" instead of cascading), so the
    // app's normal code paths can never actually produce a dangling
    // version/slot/game graph — every reachable version's slot and game are
    // guaranteed to exist. To still exercise the validate-then-mutate ordering
    // this task is about, we manufacture the scenario directly: create two real
    // versions on the same slot (so the slot's active pointer no longer matches
    // the older one), then disable FK enforcement on the connection just long
    // enough to delete the owning game row without cascading, leaving the slot
    // and both versions as dangling-but-individually-loadable rows — exactly
    // like a stale foreign version_id would look to `restore_version`.
    //
    // Two versions matter here: `create_version` itself sets the slot's
    // `active_version_id` to whatever it just created, so with only one version
    // the "before" pointer would already equal the version we try to restore —
    // making the final assertion vacuously true regardless of the bug. With two
    // versions, "before" is the *second* version's id, and we attempt to
    // restore the stale *first* one, so any mutation is actually observable.
    let h = harness("live-state");
    let game = h.create_game("Ownership Test");
    let versions_repo =
        SqliteVersionRepository::with_repository_root(h.pool.clone(), h.repo_root());

    let v1 = versions_repo
        .create_version_from_snapshot(
            game.id,
            VersionReason::Watcher,
            savegame_timeline_core::content_store::CapturedSnapshot {
                content_hash: String::new(),
                files: Vec::new(),
            },
        )
        .unwrap();
    let v2 = versions_repo
        .create_version_from_snapshot(
            game.id,
            VersionReason::Watcher,
            savegame_timeline_core::content_store::CapturedSnapshot {
                content_hash: String::new(),
                files: Vec::new(),
            },
        )
        .unwrap();
    let default_slot_id = v2.slot.id;
    let before = v2.slot.active_version_id;
    assert_eq!(
        before,
        Some(v2.version.id),
        "sanity check: creating a version activates it"
    );
    assert_ne!(
        v1.version.id, v2.version.id,
        "sanity check: the two versions must be distinct"
    );

    // Orphan the graph: disable FK enforcement on this connection just for the
    // delete, then remove the game row directly so it isn't cascaded away.
    {
        use diesel::prelude::*;
        use savegame_timeline_core::schema::games::dsl as games_dsl;
        let conn = &mut h.pool.get().unwrap();
        diesel::sql_query("PRAGMA foreign_keys = OFF")
            .execute(conn)
            .unwrap();
        diesel::delete(games_dsl::games.filter(games_dsl::id.eq(game.id)))
            .execute(conn)
            .unwrap();
    }

    // Attempt to restore the now-stale, now-orphaned first version.
    let result = h.core.restore_version(v1.version.id);
    assert!(
        result.is_err(),
        "restoring a version whose owning game no longer exists must fail"
    );

    let after: Option<i32> = {
        use diesel::prelude::*;
        use savegame_timeline_core::schema::slots::dsl as slots_dsl;
        let conn = &mut h.pool.get().unwrap();
        slots_dsl::slots
            .filter(slots_dsl::id.eq(default_slot_id))
            .select(slots_dsl::active_version_id)
            .first(conn)
            .unwrap()
    };
    assert_eq!(
        before, after,
        "a failed restore must not mutate the slot's active_version_id pointer"
    );
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

fn repo_contains_manifest_file(root: &std::path::Path, name: &str) -> bool {
    fs::read_dir(root).is_ok_and(|entries| {
        entries.filter_map(|e| e.ok()).any(|e| {
            let path = e.path();
            if path.is_dir() {
                return repo_contains_manifest_file(&path, name);
            }
            if e.file_name() != "metadata.json" {
                return false;
            }
            let Ok(metadata) = savegame_timeline_core::repository::read_metadata_json::<
                savegame_timeline_core::repository::VersionMetadata,
            >(path.parent().unwrap()) else {
                return false;
            };
            metadata.files.iter().any(|file| file.relative_path == name)
        })
    })
}

#[test]
fn test_exclusions_are_honoured_symmetrically_on_capture_and_restore() {
    // NOTE: the brief's version of this test called `h.create_game(...)` and then
    // immediately read `full.versions` — but in this codebase `save_game` only
    // creates the default slot, never a version, so that list is empty and the
    // `min_by_key(...).unwrap()` would panic. It also keyed the target version
    // off `version_number`, which is a non-unique display hint. This version
    // drives `create_slot` (the real checkpoint call site), selects the version
    // by `reason`, and asserts on on-disk contents at every step.
    let h = harness("v1-state");
    let game = h.create_game("Exclusions Test");
    let default_slot_id = h.core.get_game(game.id).unwrap().active_slot.unwrap().id;

    // Mark *.cfg as excluded for the game's tracked dir.
    {
        use diesel::prelude::*;
        use savegame_timeline_core::schema::tracked_dirs;
        let conn = &mut h.pool.get().unwrap();
        diesel::update(tracked_dirs::table.filter(tracked_dirs::game_id.eq(game.id)))
            .set(tracked_dirs::exclusions.eq(r#"["*.cfg"]"#))
            .execute(conn)
            .unwrap();
    }

    let cfg = h.source_dir.path().join("settings.cfg");
    let ini = h.source_dir.path().join("config.ini");
    fs::write(&cfg, "local-only").unwrap();
    fs::write(&ini, "ini-save-data").unwrap();

    // create_slot checkpoints the live state into the default slot, then clears
    // the live dir so the new slot starts empty. Both halves must honour the
    // exclusion.
    h.core.create_slot(game.id, "Second").unwrap();

    // 1. The excluded file must never have been captured into any snapshot.
    assert!(
        !repo_contains_manifest_file(&h.repo_root(), "settings.cfg"),
        "an excluded file must not be copied into any snapshot in the repository"
    );
    // A .ini file is real save data for many games and must be captured
    // (regression for the over-broad global exclusion list in fs_utils).
    assert!(
        repo_contains_manifest_file(&h.repo_root(), "config.ini"),
        ".ini files are real save data and must be captured into the snapshot"
    );

    // 2. The excluded file must survive the wipe that clears the live dir.
    assert_eq!(
        fs::read_to_string(&cfg).unwrap(),
        "local-only",
        "an excluded file must survive clear_tracked_dirs"
    );
    assert!(
        !h.save_file.exists(),
        "non-excluded files are cleared for the new empty slot"
    );
    assert!(
        !ini.exists(),
        "non-excluded files are cleared for the new empty slot"
    );

    let slots = h.core.get_slots(game.id).unwrap();
    let default_slot = slots.iter().find(|s| s.slot.id == default_slot_id).unwrap();
    let target = default_slot
        .versions
        .iter()
        .find(|v| v.reason == "pre_slot_create")
        .expect("create_slot checkpoints the outgoing slot");

    // Change the excluded file so a restore that clobbered it would be visible.
    fs::write(&cfg, "changed-after-capture").unwrap();

    h.core.restore_version(target.id).unwrap();

    // 3. Non-excluded files are restored from the snapshot.
    assert_eq!(
        fs::read_to_string(&h.save_file).unwrap(),
        "v1-state",
        "non-excluded files must be restored"
    );
    assert_eq!(
        fs::read_to_string(&ini).unwrap(),
        "ini-save-data",
        ".ini files must survive a capture-and-restore round trip"
    );

    // 4. The symmetry property: the excluded file is neither destroyed nor
    //    overwritten by the restore.
    assert_eq!(
        fs::read_to_string(&cfg).unwrap(),
        "changed-after-capture",
        "an excluded file must survive a restore untouched"
    );
}

/// Repeats a triggering write until the watcher has persisted `target` versions
/// or the bounded deadline expires. The retry window exceeds the 0.1s test
/// debounce, so each attempt can produce a distinct notification instead of
/// continuously resetting one debounce window.
fn write_until_version_count(
    core: &TimelineCore,
    path: &Path,
    content: &str,
    game_id: i32,
    target: usize,
    timeout: Duration,
) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        fs::write(path, content).unwrap();
        let attempt_deadline = std::cmp::min(
            deadline,
            std::time::Instant::now() + Duration::from_millis(250),
        );
        while std::time::Instant::now() < attempt_deadline {
            if core.get_game(game_id).unwrap().versions.len() >= target {
                return true;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
    }
}

fn wait_for_change_detected(
    rx: &savegame_timeline_core::detection::NotificationReceiver,
    game_id: i32,
    timeout: Duration,
) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) else {
            return false;
        };
        match rx.recv_timeout(remaining) {
            Ok(savegame_timeline_core::detection::Notification::ChangeDetected {
                game_id: received_id,
            }) if received_id == game_id => return true,
            Ok(savegame_timeline_core::detection::Notification::Error(error)) => {
                panic!("watcher reported an error: {error}")
            }
            Ok(_) => continue,
            Err(_) => return false,
        }
    }
}

/// Repeats a triggering write until its `ChangeDetected` notification arrives.
/// This handles asynchronous registration without an arbitrary pre-write sleep.
fn write_until_change_detected(
    rx: &savegame_timeline_core::detection::NotificationReceiver,
    path: &Path,
    content: &str,
    game_id: i32,
    timeout: Duration,
) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        fs::write(path, content).unwrap();
        let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) else {
            return false;
        };
        let attempt = remaining.min(Duration::from_millis(250));
        if wait_for_change_detected(rx, game_id, attempt) {
            return true;
        }
    }
}

#[test]
fn test_restore_does_not_trigger_a_spurious_watcher_version() {
    // NOTE: the brief's version of this test proves nothing. It reads
    // `full.versions` straight after `create_game`, but `save_game` never
    // creates a version, so its `min_by_key(...).unwrap()` panics. And even
    // fixed up, the watcher refuses to create a version while the newest one is
    // younger than `version_cooldown_minutes` (15 by default) — so "restore,
    // then assert no new version" passes on the cooldown alone and would keep
    // passing with watcher suppression deleted outright.
    //
    // This version backdates `created_at` so the cooldown is wide open at the
    // moment the restore's own writes land, and then asserts BOTH halves of the
    // contract: the app's writes produce nothing, and the watcher is still live
    // afterwards (suppression that never lifted would pass the first half while
    // having silently killed file watching).
    let h = harness("v1-state");
    let game = h.create_game("Suppression Test");

    // A genuine user write produces a version, proving the watcher is live.
    assert!(write_until_version_count(
        &h.core,
        &h.save_file,
        "v2-state",
        game.id,
        1,
        Duration::from_secs(30),
    ));
    assert!(wait_for_change_detected(
        &h._notif_rx,
        game.id,
        Duration::from_secs(30),
    ));
    // Exactly one version exists, so this is unambiguously it — no need to
    // order by the non-unique version_number to pick a target.
    let target = h.core.get_game(game.id).unwrap().versions[0].id;

    // Re-open the cooldown window. `run_with_paused_watcher` reloads the
    // watcher's cached games from the database (ReregisterGames) before
    // resuming, so the restore's own events are judged against these backdated
    // timestamps: nothing but the suppression itself stops them versioning.
    h.backdate_versions();

    // restore_version wipes and re-copies the tracked dir, so the app really
    // does generate remove+create events for files the watcher is watching.
    h.core.restore_version(target).unwrap();
    assert_eq!(
        fs::read_to_string(&h.save_file).unwrap(),
        "v2-state",
        "the restore must actually have rewritten the tracked directory"
    );

    // Rewriting the same bytes is a real filesystem event, but the shared
    // checkpoint path must recognize it as unchanged. Waiting for its
    // ChangeDetected notification proves the event pass completed without a
    // fixed debounce sleep.
    assert!(write_until_change_detected(
        &h._notif_rx,
        &h.save_file,
        "v2-state",
        game.id,
        Duration::from_secs(30),
    ));
    assert_eq!(
        h.core.get_game(game.id).unwrap().versions.len(),
        1,
        "a restore's own writes must not produce a watcher version"
    );

    // The other half of the contract. This also retroactively proves the
    // assertion above had teeth: the cooldown was open the whole time, so a
    // leaked event would have versioned.
    assert!(write_until_version_count(
        &h.core,
        &h.save_file,
        "user-write-after-restore",
        game.id,
        2,
        Duration::from_secs(30),
    ));
    assert!(wait_for_change_detected(
        &h._notif_rx,
        game.id,
        Duration::from_secs(30),
    ));
}

#[test]
fn test_watcher_is_resumed_when_the_paused_operation_fails() {
    // One failed restore must not disable file watching for the rest of the
    // session: `run_with_paused_watcher` has to resume (and reregister) on the
    // error path too.
    let h = harness("v1-state");
    let game = h.create_game("Resume On Error");

    assert!(write_until_version_count(
        &h.core,
        &h.save_file,
        "v2-state",
        game.id,
        1,
        Duration::from_secs(30),
    ));
    assert!(wait_for_change_detected(
        &h._notif_rx,
        game.id,
        Duration::from_secs(30),
    ));
    h.backdate_versions();

    assert!(
        h.core.restore_version(999_999).is_err(),
        "restoring a version that does not exist must fail inside the paused block"
    );

    assert!(write_until_version_count(
        &h.core,
        &h.save_file,
        "written-after-the-failure",
        game.id,
        2,
        Duration::from_secs(30),
    ));
    assert!(wait_for_change_detected(
        &h._notif_rx,
        game.id,
        Duration::from_secs(30),
    ));
}

/// Reads a slot's version ids straight from the table.
///
/// `get_slots` would answer through the very repository code under test, and
/// the facade otherwise only ever exposes the *active* slot's versions. Going
/// to the table means a delete that drops the slot row but leaves its versions
/// behind — pointing at a slot that no longer exists — cannot pass.
fn version_ids_for_slot(
    pool: &Pool<ConnectionManager<SqliteConnection>>,
    slot_id: i32,
) -> Vec<i32> {
    use diesel::prelude::*;
    use savegame_timeline_core::schema::versions::dsl as versions_dsl;

    let conn = &mut pool.get().unwrap();
    versions_dsl::versions
        .filter(versions_dsl::slot_id.eq(slot_id))
        .select(versions_dsl::id)
        .load(conn)
        .unwrap()
}

#[test]
fn test_delete_inactive_slot_drops_only_its_history_and_leaves_live_files_alone() {
    let h = harness("default-state");
    let game = h.create_game("Delete Inactive Slot");
    let default_slot_id = h.core.get_game(game.id).unwrap().active_slot.unwrap().id;

    // Leaving the default slot checkpoints "default-state" into it, so the slot
    // we delete genuinely owns history rather than being empty.
    let with_scratch = h.core.create_slot(game.id, "Scratch").unwrap();
    let scratch_slot_id = with_scratch.active_slot.as_ref().unwrap().id;
    fs::write(&h.save_file, "scratch-state").unwrap();

    let doomed_versions = version_ids_for_slot(&h.pool, default_slot_id);
    assert!(
        !doomed_versions.is_empty(),
        "precondition: the slot being deleted must own at least one version"
    );
    let survivors = version_ids_for_slot(&h.pool, scratch_slot_id);
    assert!(
        !survivors.is_empty(),
        "precondition: the surviving slot must own at least one version"
    );

    // No replacement needed: the deleted slot is not the active one.
    let after = h.core.delete_slot(default_slot_id, None).unwrap();

    assert_eq!(after.slots.len(), 1, "the deleted slot must be gone");
    assert_eq!(
        after.active_slot.as_ref().unwrap().id,
        scratch_slot_id,
        "deleting an inactive slot must not move the active slot"
    );
    assert!(
        version_ids_for_slot(&h.pool, default_slot_id).is_empty(),
        "the deleted slot's versions must be removed, not orphaned"
    );
    assert_eq!(
        version_ids_for_slot(&h.pool, scratch_slot_id),
        survivors,
        "the surviving slot's history must be untouched"
    );
    assert_eq!(
        fs::read_to_string(&h.save_file).unwrap(),
        "scratch-state",
        "deleting an inactive slot must never write to the live save directory"
    );
}

#[test]
fn test_delete_active_slot_restores_the_replacement_slots_files() {
    let h = harness("default-state");
    let game = h.create_game("Delete Active Slot");
    let default_slot_id = h.core.get_game(game.id).unwrap().active_slot.unwrap().id;

    let with_scratch = h.core.create_slot(game.id, "Scratch").unwrap();
    let scratch_slot_id = with_scratch.active_slot.as_ref().unwrap().id;
    fs::write(&h.save_file, "scratch-state").unwrap();

    let after = h
        .core
        .delete_slot(scratch_slot_id, Some(default_slot_id))
        .unwrap();

    assert_eq!(after.slots.len(), 1);
    assert_eq!(
        after.active_slot.as_ref().unwrap().id,
        default_slot_id,
        "deleting the active slot must leave the replacement active"
    );
    assert_eq!(
        fs::read_to_string(&h.save_file).unwrap(),
        "default-state",
        "the live save directory must hold the replacement slot's state, not \
         the deleted slot's leftovers"
    );
    assert!(
        version_ids_for_slot(&h.pool, scratch_slot_id).is_empty(),
        "the deleted slot's versions must be removed, not orphaned"
    );
    assert!(
        !version_ids_for_slot(&h.pool, default_slot_id).is_empty(),
        "the replacement slot must keep the history it is being restored from"
    );
}

#[test]
fn test_delete_slot_refuses_to_leave_a_game_without_a_coherent_active_slot() {
    let h = harness("default-state");
    let game = h.create_game("Delete Slot Guards");
    let default_slot_id = h.core.get_game(game.id).unwrap().active_slot.unwrap().id;

    // Every guard below is asserted on its message rather than on a bare
    // `is_err`. The four rejected calls all target the active slot, so any one
    // guard is enough to make `is_err` true for all of them — removing three of
    // the four left this test green until the messages were pinned down.
    // `{:#}` renders the whole anyhow chain: plain `to_string` would show only
    // the outermost context and hide the guard that actually fired.
    let refusal = |slot_id, replacement| {
        let err = h
            .core
            .delete_slot(slot_id, replacement)
            .expect_err("delete must be rejected");
        format!("{err:#}")
    };

    let err = refusal(default_slot_id, None);
    assert!(
        err.contains("must keep at least one slot"),
        "the last remaining slot must not be deletable, got: {err}"
    );

    let with_scratch = h.core.create_slot(game.id, "Scratch").unwrap();
    let scratch_slot_id = with_scratch.active_slot.as_ref().unwrap().id;

    let err = refusal(scratch_slot_id, None);
    assert!(
        err.contains("requires a replacement slot"),
        "deleting the active slot without a replacement must fail, got: {err}"
    );

    let err = refusal(scratch_slot_id, Some(scratch_slot_id));
    assert!(
        err.contains("must not be the slot being deleted"),
        "the slot being deleted must not be its own replacement, got: {err}"
    );

    let err = refusal(scratch_slot_id, Some(999_999));
    assert!(
        err.contains("does not belong to this game"),
        "a replacement slot from another game must be rejected, got: {err}"
    );

    let game_after = h.core.get_game(game.id).unwrap();
    assert_eq!(
        game_after.slots.len(),
        2,
        "no rejected delete may have removed a slot"
    );
    assert_eq!(
        game_after.active_slot.as_ref().unwrap().id,
        scratch_slot_id,
        "no rejected delete may have moved the active slot"
    );
}

#[test]
fn test_rename_slot_keeps_identity_and_history_and_rejects_collisions() {
    let h = harness("default-state");
    let game = h.create_game("Rename Slot");
    let default_slot_id = h.core.get_game(game.id).unwrap().active_slot.unwrap().id;
    h.core.create_slot(game.id, "Scratch").unwrap();

    let slug_before = h
        .core
        .get_slots(game.id)
        .unwrap()
        .into_iter()
        .find(|s| s.slot.id == default_slot_id)
        .unwrap()
        .slot
        .folder_slug;
    let versions_before = version_ids_for_slot(&h.pool, default_slot_id);

    let after = h.core.rename_slot(default_slot_id, "Story Mode").unwrap();
    let renamed = after
        .slots
        .iter()
        .find(|s| s.id == default_slot_id)
        .unwrap();

    assert_eq!(renamed.name, "Story Mode");
    assert_eq!(
        renamed.folder_slug, slug_before,
        "a rename must not move the slot's repository directory: the slug is \
         anchored on the slot UUID and already names every version beneath it"
    );
    assert_eq!(
        version_ids_for_slot(&h.pool, default_slot_id),
        versions_before,
        "a rename must not disturb the slot's history"
    );

    // Asserted on the message: a unique index on (game_id, name_normalized)
    // means a bare `is_err` is satisfied by the raw SQL constraint failure, and
    // stays green with the explicit check — and its readable message — deleted.
    let collision = format!(
        "{:#}",
        h.core
            .rename_slot(default_slot_id, "Scratch")
            .expect_err("rename onto a sibling's name must be rejected")
    );
    assert!(
        collision.contains("already exists for this game"),
        "renaming onto a sibling slot's name must be rejected by name, got: {collision}"
    );
    assert!(
        h.core.rename_slot(default_slot_id, "   ").is_err(),
        "an empty slot name must be rejected"
    );
    assert_eq!(
        h.core
            .get_slots(game.id)
            .unwrap()
            .into_iter()
            .find(|s| s.slot.id == default_slot_id)
            .unwrap()
            .slot
            .name,
        "Story Mode",
        "a rejected rename must leave the previous name in place"
    );
}
