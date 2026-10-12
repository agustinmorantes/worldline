mod common;

use common::Device;
use std::fs;
use worldline_core::detection::BranchFrom;
use worldline_core::history::CheckpointOutcome;
use worldline_core::models::GameForm;
use worldline_core::{OpenOptions, WorldlineCore};

#[test]
fn creating_a_game_captures_its_saves_and_unchanged_checkpoints_are_no_ops() {
    let device = Device::new();
    device.write("save.dat", "level 1");
    let game = device.add_game("Hollow Knight");
    let first = device.active_version(game.game.id);
    assert_eq!(first.reason, "initial");

    assert!(matches!(
        device.core.checkpoint(game.game.id).unwrap(),
        CheckpointOutcome::Unchanged(_)
    ));
    device.write("save.dat", "level 2");
    let second = device.checkpoint(game.game.id);
    assert_ne!(second.id, first.id);
    assert_eq!(second.parent_id, Some(first.id));
    assert_eq!(device.game(game.game.id).versions.len(), 2);
}

#[test]
fn an_empty_folder_without_history_records_nothing() {
    let device = Device::new();
    let game = device.add_game("Empty");
    assert!(game.versions.is_empty());
    assert!(matches!(
        device.core.checkpoint(game.game.id).unwrap(),
        CheckpointOutcome::Empty
    ));
}

/// Keeps and returns the outcome holding the version as tagged, which must succeed.
fn keep(device: &Device, game_id: i32) -> (CheckpointOutcome, Option<BranchFrom>) {
    let kept = device
        .core
        .keep(game_id, "Kept in game, 21:04", || true)
        .unwrap()
        .expect("the keep stands");
    let outcome = match (kept.checkpoint, kept.tagged) {
        (CheckpointOutcome::Created(_), Some(tagged)) => {
            CheckpointOutcome::Created(tagged.unwrap())
        }
        (CheckpointOutcome::Unchanged(_), Some(tagged)) => {
            CheckpointOutcome::Unchanged(tagged.unwrap())
        }
        (outcome, tagged) => {
            assert!(tagged.is_none(), "nothing captured, nothing tagged");
            outcome
        }
    };
    (outcome, kept.branch_from)
}

fn tag_of(device: &Device, version_id: i32) -> Option<String> {
    device.core.get_version(version_id).unwrap().version.tag
}

#[test]
fn keep_captures_changed_saves_now_and_tags_the_new_version() {
    let device = Device::new();
    device.write("save.dat", "level 1");
    let game = device.add_game("Hollow Knight");
    device.write("save.dat", "level 2");
    let (outcome, branch_from) = keep(&device, game.game.id);
    let CheckpointOutcome::Created(kept) = outcome else {
        panic!("a new version: {outcome:?}");
    };
    assert_eq!(kept.tag.as_deref(), Some("Kept in game, 21:04"));
    assert_eq!(tag_of(&device, kept.id), kept.tag);
    assert_eq!(device.active_version(game.game.id).id, kept.id);
    assert_eq!(branch_from, None);
    // No cooldown between presses: the next change is captured at once.
    device.write("save.dat", "level 3");
    assert!(matches!(
        keep(&device, game.game.id).0,
        CheckpointOutcome::Created(v) if v.id != kept.id
    ));
}

#[test]
fn keep_tags_the_active_version_when_the_saves_havent_changed() {
    let device = Device::new();
    device.write("save.dat", "old");
    let game = device.add_game("Celeste");
    let old = device.active_version(game.game.id);
    device.write("save.dat", "new");
    let latest = device.checkpoint(game.game.id);
    // After a restore the saves match the old version, the one the player is on: it's the one
    // kept, not the slot's newest, which is on the branch they left.
    device.core.restore_version(old.id).unwrap();
    let versions = device.game(game.game.id).versions.len();

    let (outcome, _) = keep(&device, game.game.id);
    let CheckpointOutcome::Unchanged(kept) = outcome else {
        panic!("unchanged: {outcome:?}");
    };
    assert_eq!(kept.id, old.id);
    assert_eq!(
        tag_of(&device, old.id).as_deref(),
        Some("Kept in game, 21:04")
    );
    assert_eq!(tag_of(&device, latest.id), None);
    assert_eq!(device.active_version(game.game.id).id, old.id);
    assert_eq!(device.game(game.game.id).versions.len(), versions);
}

#[test]
fn keep_leaves_an_existing_tag_alone() {
    let device = Device::new();
    device.write("save.dat", "x");
    let game = device.add_game("Hades");
    let tip = device.active_version(game.game.id);
    device
        .core
        .tag_version(tip.id, "Before the boss".into())
        .unwrap();

    let (outcome, _) = keep(&device, game.game.id);
    assert_eq!(
        outcome.version().and_then(|v| v.tag.as_deref()),
        Some("Before the boss")
    );
    assert_eq!(tag_of(&device, tip.id).as_deref(), Some("Before the boss"));
}

#[test]
fn a_rename_waits_for_the_keep_lock_and_keep_then_leaves_the_name_alone() {
    let device = Device::new();
    device.write("save.dat", "x");
    let game = device.add_game("Hades");
    let tip = device.active_version(game.game.id);
    let renamed = std::sync::atomic::AtomicBool::new(false);
    std::thread::scope(|scope| {
        // Held as keep holds it, between its capture and its tag.
        worldline_core::store::with_lock(device.core.repository_root(), || {
            let rename = scope.spawn(|| {
                device
                    .core
                    .tag_version(tip.id, "Before the boss".into())
                    .unwrap();
                renamed.store(true, std::sync::atomic::Ordering::SeqCst);
            });
            std::thread::sleep(std::time::Duration::from_millis(300));
            assert!(!renamed.load(std::sync::atomic::Ordering::SeqCst));
            assert!(!rename.is_finished(), "the rename waits for the lock");
            Ok(())
        })
        .unwrap();
    });
    assert!(renamed.load(std::sync::atomic::Ordering::SeqCst));
    let (outcome, _) = keep(&device, game.game.id);
    assert_eq!(
        outcome.version().and_then(|v| v.tag.as_deref()),
        Some("Before the boss")
    );
    assert_eq!(tag_of(&device, tip.id).as_deref(), Some("Before the boss"));
}

#[test]
fn keep_with_nothing_to_capture_or_a_blank_tag_changes_nothing() {
    let device = Device::new();
    let game = device.add_game("Empty");
    assert!(matches!(
        keep(&device, game.game.id).0,
        CheckpointOutcome::Empty
    ));
    assert!(device.game(game.game.id).versions.is_empty());

    let device = Device::new();
    device.write("save.dat", "x");
    let game = device.add_game("Untagged");
    let tip = device.active_version(game.game.id);
    device.write("save.dat", "y");
    assert!(device.core.keep(game.game.id, "  ", || true).is_err());
    // Refused before anything was captured or tagged.
    assert_eq!(device.game(game.game.id).versions.len(), 1);
    assert_eq!(tag_of(&device, tip.id), None);
}

#[test]
fn a_keep_that_no_longer_stands_under_the_lock_changes_nothing() {
    let device = Device::new();
    device.write("save.dat", "x");
    let game = device.add_game("Untagged");
    let tip = device.active_version(game.game.id);
    device.write("save.dat", "y");
    let kept = device
        .core
        .keep(game.game.id, "Kept in game, 21:04", || false)
        .unwrap();
    assert!(kept.is_none());
    assert_eq!(device.game(game.game.id).versions.len(), 1);
    assert_eq!(tag_of(&device, tip.id), None);
}

#[test]
fn restore_checkpoints_uncaptured_progress_before_overwriting_it() {
    let device = Device::new();
    device.write("save.dat", "old");
    let game = device.add_game("Celeste");
    let old = device.active_version(game.game.id);
    device.write("save.dat", "captured");
    device.checkpoint(game.game.id);
    // Progress made during the watcher cooldown, never captured.
    device.write("save.dat", "uncaptured");

    device.core.restore_version(old.id).unwrap();

    assert_eq!(device.read("save.dat").as_deref(), Some("old"));
    let game = device.game(game.game.id);
    let pre_restore = game
        .versions
        .iter()
        .find(|v| v.reason == "pre_restore")
        .expect("pre-restore checkpoint");
    device.core.restore_version(pre_restore.id).unwrap();
    assert_eq!(device.read("save.dat").as_deref(), Some("uncaptured"));
}

#[test]
fn restoring_the_active_version_keeps_the_unrecorded_saves_and_goes_back() {
    let device = Device::new();
    device.write("save.dat", "before the death");
    let game = device.add_game("Silksong");
    let active = device.active_version(game.game.id);
    // Saved again inside the cooldown: not recorded yet.
    device.write("save.dat", "after the death");

    device.core.restore_version(active.id).unwrap();

    assert_eq!(device.read("save.dat").as_deref(), Some("before the death"));
    assert_eq!(device.active_version(game.game.id).id, active.id);
    let game = device.game(game.game.id);
    assert_eq!(game.versions.len(), 2);
    let kept = game
        .versions
        .iter()
        .find(|v| v.reason == "pre_restore")
        .expect("pre-restore checkpoint");
    assert_eq!(kept.parent_id, Some(active.id));
    let out = tempfile::tempdir().unwrap();
    device.core.extract_version(kept.id, out.path()).unwrap();
    let dir = &game.tracked_dirs[0].folder_slug;
    assert_eq!(
        fs::read_to_string(out.path().join(dir).join("save.dat")).unwrap(),
        "after the death"
    );
}

#[test]
fn a_damaged_chunk_aborts_restore_before_live_files_change() {
    let device = Device::new();
    device.write("save.dat", "old");
    let game = device.add_game("Hades");
    let old = device.active_version(game.game.id);
    device.write("save.dat", "new");
    device.checkpoint(game.game.id);
    let chunks = device.core.repository_root().join("chunks");
    for shard in fs::read_dir(&chunks).unwrap() {
        for chunk in fs::read_dir(shard.unwrap().path()).unwrap() {
            fs::write(chunk.unwrap().path(), b"garbage").unwrap();
        }
    }
    assert!(device.core.restore_version(old.id).is_err());
    assert_eq!(device.read("save.dat").as_deref(), Some("new"));
}

#[test]
fn exclusions_are_neither_captured_nor_wiped() {
    let device = Device::new();
    device.write("save.dat", "one");
    device.write("settings.cfg", "mine");
    let game = device.add_game("Excluded");
    let dir = &game.tracked_dirs[0];
    // Exclusions are edited in the files; reindex projects them.
    let dir_meta = device
        .core
        .repository_root()
        .join("games")
        .join(&game.game.folder_slug)
        .join("tracked-dirs")
        .join(&dir.folder_slug)
        .join("metadata.json");
    let mut meta: serde_json::Value =
        serde_json::from_slice(&fs::read(&dir_meta).unwrap()).unwrap();
    meta["exclusions"] = serde_json::json!(["*.cfg"]);
    meta["updated_at"] = serde_json::json!(chrono::Utc::now());
    fs::write(&dir_meta, serde_json::to_vec(&meta).unwrap()).unwrap();
    device.core.reindex().unwrap();

    device.write("save.dat", "two");
    let captured = device.checkpoint(game.game.id);
    device.write("settings.cfg", "changed locally");
    let first = device.game(game.game.id).versions.last().unwrap().clone();
    device.core.restore_version(first.id).unwrap();
    assert_eq!(device.read("save.dat").as_deref(), Some("one"));
    assert_eq!(
        device.read("settings.cfg").as_deref(),
        Some("changed locally")
    );
    assert_ne!(captured.id, first.id);
}

#[test]
fn slots_keep_independent_histories() {
    let device = Device::new();
    device.write("save.dat", "main run");
    let game = device.add_game("Slots");
    let id = game.game.id;
    let main_slot = game.active_slot.clone().unwrap();

    let game = device.core.create_slot(id, "  Speedrun ").unwrap();
    let speedrun = game.active_slot.clone().unwrap();
    assert_eq!(speedrun.name, "Speedrun");
    assert_eq!(device.read("save.dat"), None, "a new slot starts empty");
    device.write("save.dat", "speedrun progress");
    device.checkpoint(id);

    device.core.switch_slot(id, main_slot.id).unwrap();
    assert_eq!(device.read("save.dat").as_deref(), Some("main run"));
    device.core.switch_slot(id, speedrun.id).unwrap();
    assert_eq!(
        device.read("save.dat").as_deref(),
        Some("speedrun progress")
    );

    assert!(
        device.core.create_slot(id, "speedrun").is_err(),
        "names are unique per game"
    );
    assert!(device.core.rename_slot(main_slot.id, "SPEEDRUN").is_err());
    device.core.rename_slot(main_slot.id, "Story").unwrap();
    assert!(
        device.core.delete_slot(speedrun.id, None).is_err(),
        "active slot needs a replacement"
    );
    let game = device
        .core
        .delete_slot(speedrun.id, Some(main_slot.id))
        .unwrap();
    assert_eq!(game.slots.len(), 1);
    assert_eq!(device.read("save.dat").as_deref(), Some("main run"));
    assert!(
        device.core.delete_slot(main_slot.id, None).is_err(),
        "the last slot stays"
    );
}

#[test]
fn deleting_a_game_with_history_works_and_is_not_resurrected_by_reindex() {
    let device = Device::new();
    device.write("save.dat", "x");
    let game = device.add_game("Doomed");
    device.core.create_slot(game.game.id, "Other").unwrap();
    device.core.delete_game(game.game.id).unwrap();
    assert!(device.core.get_games().unwrap().is_empty());
    device.core.reindex().unwrap();
    assert!(device.core.get_games().unwrap().is_empty());
}

#[test]
fn renaming_a_game_keeps_its_folder_and_history() {
    let device = Device::new();
    device.write("save.dat", "x");
    let game = device.add_game("Old Name");
    let renamed = device
        .core
        .save_game(
            GameForm {
                id: Some(game.game.id),
                title: "New Name".into(),
            },
            None,
        )
        .unwrap();
    assert_eq!(renamed.folder_slug, game.game.folder_slug);
    assert_eq!(device.game(game.game.id).versions.len(), 1);
    assert!(
        device.add_game_result("new   name").is_err(),
        "titles are unique"
    );
}

impl Device {
    fn add_game_result(&self, title: &str) -> anyhow::Result<worldline_core::models::Game> {
        self.core.save_game(
            GameForm {
                id: None,
                title: title.into(),
            },
            Some(vec![]),
        )
    }
}

#[test]
fn the_index_is_rebuilt_from_repository_files() {
    let device = Device::new();
    device.write("save.dat", "x");
    let game = device.add_game("Rebuilt");
    device.write("save.dat", "y");
    let tip = device.checkpoint(game.game.id);
    device.core.tag_version(tip.id, "boss".into()).unwrap();
    let Device { core, data, saves } = device;
    drop(core);
    fs::remove_file(data.path().join("database.db")).unwrap();

    let (core, _) = WorldlineCore::open(data.path(), OpenOptions::default()).unwrap();
    let games = core.get_games().unwrap();
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].game.title, "Rebuilt");
    let versions = core
        .get_slot(games[0].active_slot.as_ref().unwrap().id)
        .unwrap()
        .versions;
    assert_eq!(versions.len(), 2);
    assert!(versions.iter().any(|v| v.tag.as_deref() == Some("boss")));
    drop(saves);
}

#[test]
fn reopening_a_data_dir_keeps_its_data() {
    let device = Device::new();
    device.write("save.dat", "x");
    device.add_game("Kept");
    let Device { core, data, saves } = device;
    drop(core);
    let (core, _) = WorldlineCore::open(data.path(), OpenOptions::default()).unwrap();
    assert!(core.moved_old_data().is_none());
    assert_eq!(core.get_games().unwrap().len(), 1);
    drop(saves);
}

#[test]
fn pre_release_data_is_moved_aside_never_deleted() {
    let data = tempfile::tempdir().unwrap();
    fs::create_dir_all(data.path().join("repository")).unwrap();
    fs::write(
        data.path().join("repository").join("metadata.json"),
        br#"{"format_version":2,"created_at":"2026-01-01T00:00:00Z","app":"worldline"}"#,
    )
    .unwrap();
    let (core, _) = WorldlineCore::open(data.path(), OpenOptions::default()).unwrap();
    let backup = core.moved_old_data().expect("old data moved").to_path_buf();
    assert!(backup.join("repository").join("metadata.json").exists());
    assert!(core.get_games().unwrap().is_empty());
}

#[test]
fn unknown_database_migrations_do_not_move_current_data_aside() {
    use diesel::{Connection, RunQueryDsl};

    let device = Device::new();
    device.write("save.dat", "precious");
    device.add_game("Kept");
    let Device { core, data, saves } = device;
    drop(core);
    let database = data.path().join("database.db");
    let mut conn = diesel::SqliteConnection::establish(&database.to_string_lossy()).unwrap();
    diesel::sql_query(
        "INSERT INTO __diesel_schema_migrations (version) VALUES ('209901010000000000')",
    )
    .execute(&mut conn)
    .unwrap();
    drop(conn);
    let before = fs::read(&database).unwrap();
    let metadata = data.path().join("repository/metadata.json");
    let store_before = fs::read(&metadata).unwrap();

    let error = WorldlineCore::open(data.path(), OpenOptions::default())
        .err()
        .expect("unknown migrations must stop startup");
    assert!(error.to_string().contains("209901010000000000"));
    assert_eq!(fs::read(database).unwrap(), before);
    assert_eq!(fs::read(metadata).unwrap(), store_before);
    assert!(fs::read_dir(data.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("backup-pre-")
    }));
    assert_eq!(
        fs::read_to_string(saves.path().join("Game Saves/save.dat")).unwrap(),
        "precious"
    );
}

#[test]
fn newer_store_formats_are_not_moved_aside() {
    let data = tempfile::tempdir().unwrap();
    fs::create_dir_all(data.path().join("repository")).unwrap();
    let metadata = data.path().join("repository/metadata.json");
    let contents = br#"{"format_version":5,"created_at":"2026-01-01T00:00:00Z","app":"worldline"}"#;
    fs::write(&metadata, contents).unwrap();

    assert!(WorldlineCore::open(data.path(), OpenOptions::default()).is_err());
    assert_eq!(fs::read(metadata).unwrap(), contents);
    assert_eq!(fs::read_dir(data.path()).unwrap().count(), 1);
}

#[test]
fn extract_rebuilds_a_version_without_touching_live_saves() {
    let device = Device::new();
    device.write("nested/save.dat", "extract me");
    let game = device.add_game("Extract");
    let version = device.active_version(game.game.id);
    device.write("nested/save.dat", "live");
    let out = tempfile::tempdir().unwrap();
    assert_eq!(
        device.core.extract_version(version.id, out.path()).unwrap(),
        1
    );
    let dir = &game.tracked_dirs[0].folder_slug;
    assert_eq!(
        fs::read_to_string(out.path().join(dir).join("nested/save.dat")).unwrap(),
        "extract me"
    );
    assert_eq!(device.read("nested/save.dat").as_deref(), Some("live"));
    assert!(device.core.check_integrity(true).unwrap().is_ok());
}

#[test]
fn recapture_repairs_a_chunk_that_verification_found_corrupt() {
    let device = Device::new();
    device.write("save.dat", "precious");
    let game = device.add_game("Repaired");
    let store = worldline_core::store::Store::open_existing(device.core.repository_root()).unwrap();
    let (hash, _) = store.chunk_sizes().unwrap().into_iter().next().unwrap();
    fs::write(store.chunk_path(&hash).unwrap(), b"garbage").unwrap();

    let report = device.core.check_integrity(true).unwrap();
    assert_eq!(report.corrupt_chunks, std::slice::from_ref(&hash));

    // The live saves are intact, so the next checkpoint republishes the chunk.
    device.write("save.dat", "precious");
    device.core.checkpoint(game.game.id).unwrap();
    assert!(device.core.check_integrity(true).unwrap().is_ok());
}

#[test]
fn restoring_into_a_retired_folder_checkpoints_it_first() {
    let device = Device::new();
    let other = tempfile::tempdir().unwrap();
    device.write("save.dat", "a1");
    fs::write(other.path().join("save.dat"), "b1").unwrap();
    let dirs = |paths: &[&std::path::Path]| {
        Some(
            paths
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect(),
        )
    };
    let form = || GameForm {
        id: None,
        title: "Retired".into(),
    };
    let id = device
        .core
        .save_game(form(), dirs(&[&device.save_dir(), other.path()]))
        .unwrap()
        .id;
    let old = device.active_version(id);
    device
        .core
        .save_game(
            GameForm {
                id: Some(id),
                ..form()
            },
            dirs(&[&device.save_dir()]),
        )
        .unwrap();
    fs::write(other.path().join("save.dat"), "uncaptured retired progress").unwrap();

    device.core.restore_version(old.id).unwrap();

    assert_eq!(
        fs::read_to_string(other.path().join("save.dat")).unwrap(),
        "b1"
    );
    let pre_restore = device
        .game(id)
        .versions
        .into_iter()
        .find(|v| v.reason == "pre_restore")
        .expect("pre-restore checkpoint");
    device.core.restore_version(pre_restore.id).unwrap();
    assert_eq!(
        fs::read_to_string(other.path().join("save.dat")).unwrap(),
        "uncaptured retired progress"
    );
}

#[test]
fn nested_tracked_folders_are_rejected() {
    let device = Device::new();
    device.write("inner/save.dat", "x");
    device.add_game("Outer");
    let inner = device.save_dir().join("inner");
    let result = device.core.save_game(
        GameForm {
            id: None,
            title: "Inner".into(),
        },
        Some(vec![inner.to_string_lossy().into_owned()]),
    );
    assert!(result.is_err());
}

#[test]
fn a_dot_dot_alias_of_a_tracked_folder_is_rejected() {
    let device = Device::new();
    device.write("inner/save.dat", "x");
    device.add_game("Outer");
    let alias = device.saves.path().join("alias");
    std::fs::create_dir_all(&alias).unwrap();
    let inner = alias.join("..").join("Game Saves").join("inner");
    let result = device.core.save_game(
        GameForm {
            id: None,
            title: "Inner".into(),
        },
        Some(vec![inner.to_string_lossy().into_owned()]),
    );
    assert!(result.is_err());
}

#[test]
fn reopening_reconciles_the_index_with_newer_files() {
    let device = Device::new();
    device.write("save.dat", "x");
    let game = device.add_game("Before");
    let store = worldline_core::store::Store::open_existing(device.core.repository_root()).unwrap();
    let mut meta = store.catalog().unwrap()[0].meta.clone();
    meta.title = "After".into();
    meta.updated_at += std::time::Duration::from_secs(1);
    store.write_game(&meta).unwrap();

    let Device { core, data, saves } = device;
    drop(core);
    let (core, _) = WorldlineCore::open(data.path(), OpenOptions::default()).unwrap();
    assert_eq!(core.get_game(game.game.id).unwrap().game.title, "After");
    drop(saves);
}

#[test]
fn deep_integrity_rebuilds_files_and_flags_a_wrong_size() {
    let device = Device::new();
    device.write("save.dat", "sized");
    device.add_game("Sized");
    let store = worldline_core::store::Store::open_existing(device.core.repository_root()).unwrap();
    let record = store.catalog().unwrap().pop().unwrap();
    let slot = &record.slots[0];
    let mut version = slot.versions[0].clone();
    version.files[0].size += 1;
    let path = store
        .game_dir(&record.meta.folder_slug)
        .join("slots")
        .join(&slot.meta.folder_slug)
        .join("versions")
        .join(&version.folder_slug)
        .join("metadata.json");
    worldline_core::store::write_json(&path, &version).unwrap();

    let report = device.core.check_integrity(true).unwrap();
    assert!(!report.is_ok());
    assert_eq!(report.damaged_files.len(), 1);
    assert!(report.corrupt_chunks.is_empty());
}

#[cfg(windows)]
#[test]
fn a_save_the_game_holds_fails_the_restore_without_changing_anything() {
    use std::os::windows::fs::OpenOptionsExt;
    use worldline_core::error::{ErrorKind, error_kind};
    let device = Device::new();
    device.write("a.dat", "old a");
    device.write("b.dat", "old b");
    let game = device.add_game("Silksong");
    let old = device.active_version(game.game.id);
    device.write("a.dat", "new a");
    device.write("b.dat", "new b");
    device.checkpoint(game.game.id);
    // Like a running game: others may read the file but not delete it.
    const FILE_SHARE_READ: u32 = 1;
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(device.save_dir().join("b.dat"))
        .unwrap();

    let error = device.core.restore_version(old.id).unwrap_err();

    assert_eq!(error_kind(&error), Some(ErrorKind::SavesInUse));
    drop(held);
    assert_eq!(device.read("a.dat").as_deref(), Some("new a"));
    assert_eq!(device.read("b.dat").as_deref(), Some("new b"));
    let txn = device.data.path().join("txn");
    assert!(fs::read_dir(txn).map_or(true, |mut left| left.next().is_none()));
}

#[test]
fn opening_clears_what_an_interrupted_restore_left() {
    let data = tempfile::tempdir().unwrap();
    let staged = data.path().join("txn").join("cut").join("new").join("0");
    fs::create_dir_all(&staged).unwrap();
    fs::write(staged.join("save.dat"), "half").unwrap();

    let (core, _) = WorldlineCore::open(data.path(), OpenOptions::default()).unwrap();

    assert!(!data.path().join("txn").join("cut").exists());
    assert_eq!(core.recovered_restores(), 0, "nothing live was touched");
}

#[cfg(windows)]
#[test]
fn a_rollback_held_up_by_the_game_finishes_by_itself_and_blocks_capture_until_then() {
    use std::os::windows::fs::OpenOptionsExt;
    use worldline_core::error::{ErrorKind, error_kind};
    let device = Device::new();
    device.write("a.dat", "mine");
    let game = device.add_game("Silksong");
    let versions = device.game(game.game.id).versions.len();
    // A write was undone except for a.dat, which the game still holds aside.
    let txn = device.data.path().join("txn");
    let aside = txn.join("stuck").join("old").join("0");
    fs::create_dir_all(&aside).unwrap();
    fs::rename(device.save_dir().join("a.dat"), aside.join("a.dat")).unwrap();
    let journal = serde_json::json!({
        "phase": "moving_aside",
        "roots": [game.tracked_dirs[0].path],
        "targets": [],
    });
    fs::write(txn.join("stuck.json"), journal.to_string()).unwrap();
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(aside.join("a.dat"))
        .unwrap();

    let error = device.core.checkpoint(game.game.id).unwrap_err();
    assert_eq!(error_kind(&error), Some(ErrorKind::RestoreIncomplete));
    assert_eq!(
        device.game(game.game.id).versions.len(),
        versions,
        "the half-written folder isn't recorded"
    );

    drop(held);
    device.core.checkpoint(game.game.id).unwrap();

    assert_eq!(device.read("a.dat").as_deref(), Some("mine"));
    assert_eq!(device.game(game.game.id).versions.len(), versions);
    assert!(fs::read_dir(&txn).unwrap().next().is_none());
}

#[test]
fn adding_the_same_game_twice_at_once_adds_it_once() {
    let device = Device::new();
    device.write("save.dat", "x");
    for round in 0..20 {
        let title = format!("Race {round}");
        let barrier = std::sync::Barrier::new(2);
        let added = std::thread::scope(|scope| {
            let add = || {
                barrier.wait();
                device
                    .core
                    .save_game(
                        GameForm {
                            id: None,
                            title: title.clone(),
                        },
                        Some(vec![]),
                    )
                    .is_ok()
            };
            let (a, b) = (scope.spawn(add), scope.spawn(add));
            [a.join().unwrap(), b.join().unwrap()]
        });
        assert_eq!(added.iter().filter(|&&ok| ok).count(), 1, "round {round}");
    }
}
