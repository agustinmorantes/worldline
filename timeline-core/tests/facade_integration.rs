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
use savegame_timeline_core::persistence::{GameRepository, VersionRepository};
use savegame_timeline_core::repository::VersionReason;
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
        h.working_base.path(),
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

    let (core, notif_rx) = TimelineCore::new(
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
        _notif_rx: notif_rx,
    }
}

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
    let versions_repo = SqliteVersionRepository::with_repository_root(
        h.pool.clone(),
        h.working_base.path().to_path_buf(),
    );

    let v1 = versions_repo
        .create_version(game.id, VersionReason::Watcher)
        .unwrap();
    let v2 = versions_repo
        .create_version(game.id, VersionReason::Watcher)
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

/// Recursively checks whether any file named `name` exists anywhere under `root`.
/// Used to assert on what actually landed in the repository on disk, rather
/// than trusting a database field or a return value.
fn repo_contains_file(root: &std::path::Path, name: &str) -> bool {
    fs::read_dir(root).is_ok_and(|entries| {
        entries.filter_map(|e| e.ok()).any(|e| {
            let path = e.path();
            if path.is_dir() {
                repo_contains_file(&path, name)
            } else {
                e.file_name().to_string_lossy() == name
            }
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
        !repo_contains_file(h.working_base.path(), "settings.cfg"),
        "an excluded file must not be copied into any snapshot in the repository"
    );
    // A .ini file is real save data for many games and must be captured
    // (regression for the over-broad global exclusion list in fs_utils).
    assert!(
        repo_contains_file(h.working_base.path(), "config.ini"),
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

/// Polls `get_game` until the active slot holds at least `target` versions,
/// giving up after `timeout`. Returns the count actually observed.
fn wait_for_version_count(
    core: &TimelineCore,
    game_id: i32,
    target: usize,
    timeout: Duration,
) -> usize {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let count = core.get_game(game_id).unwrap().versions.len();
        if count >= target || std::time::Instant::now() >= deadline {
            return count;
        }
        std::thread::sleep(Duration::from_millis(50));
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
    // save_game registers the tracked dir with the watcher asynchronously; give
    // that registration time to land, or the first write is invisible.
    std::thread::sleep(Duration::from_millis(300));

    // A genuine user write produces a version, proving the watcher is live.
    fs::write(&h.save_file, "v2-state").unwrap();
    assert_eq!(
        wait_for_version_count(&h.core, game.id, 1, Duration::from_secs(5)),
        1,
        "a genuine user write must produce a version"
    );
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

    // Well past the harness's 0.1s debounce window and the settle delay.
    std::thread::sleep(Duration::from_millis(800));
    assert_eq!(
        h.core.get_game(game.id).unwrap().versions.len(),
        1,
        "a restore's own writes must not produce a watcher version"
    );

    // The other half of the contract. This also retroactively proves the
    // assertion above had teeth: the cooldown was open the whole time, so a
    // leaked event would have versioned.
    fs::write(&h.save_file, "user-write-after-restore").unwrap();
    assert_eq!(
        wait_for_version_count(&h.core, game.id, 2, Duration::from_secs(5)),
        2,
        "the watcher must still be live after a restore"
    );
}

#[test]
fn test_watcher_is_resumed_when_the_paused_operation_fails() {
    // One failed restore must not disable file watching for the rest of the
    // session: `run_with_paused_watcher` has to resume (and reregister) on the
    // error path too.
    let h = harness("v1-state");
    let game = h.create_game("Resume On Error");
    std::thread::sleep(Duration::from_millis(300));

    fs::write(&h.save_file, "v2-state").unwrap();
    assert_eq!(
        wait_for_version_count(&h.core, game.id, 1, Duration::from_secs(5)),
        1,
        "a genuine user write must produce a version"
    );
    h.backdate_versions();

    assert!(
        h.core.restore_version(999_999).is_err(),
        "restoring a version that does not exist must fail inside the paused block"
    );

    // Resume/ReregisterGames are processed asynchronously by the watcher thread,
    // and a failing restore returns almost instantly — give the re-registration
    // the same grace as the initial one. (A watcher that never resumed would
    // stay paused forever, so no amount of waiting could rescue this assertion.)
    std::thread::sleep(Duration::from_millis(300));

    fs::write(&h.save_file, "written-after-the-failure").unwrap();
    assert_eq!(
        wait_for_version_count(&h.core, game.id, 2, Duration::from_secs(5)),
        2,
        "the watcher must be resumed even when the paused operation returns Err"
    );
}
