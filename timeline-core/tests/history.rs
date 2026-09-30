mod common;

use common::Device;
use savegame_timeline_core::history::CheckpointOutcome;
use savegame_timeline_core::models::GameForm;
use savegame_timeline_core::{OpenOptions, TimelineCore};
use std::fs;

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
    fn add_game_result(&self, title: &str) -> anyhow::Result<savegame_timeline_core::models::Game> {
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

    let (core, _) = TimelineCore::open(data.path(), OpenOptions::default()).unwrap();
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
    let (core, _) = TimelineCore::open(data.path(), OpenOptions::default()).unwrap();
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
        br#"{"format_version":2,"created_at":"2026-01-01T00:00:00Z","app":"savegame-timeline"}"#,
    )
    .unwrap();
    let (core, _) = TimelineCore::open(data.path(), OpenOptions::default()).unwrap();
    let backup = core.moved_old_data().expect("old data moved").to_path_buf();
    assert!(backup.join("repository").join("metadata.json").exists());
    assert!(core.get_games().unwrap().is_empty());
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
    let store =
        savegame_timeline_core::store::Store::open_existing(device.core.repository_root()).unwrap();
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
fn reopening_reconciles_the_index_with_newer_files() {
    let device = Device::new();
    device.write("save.dat", "x");
    let game = device.add_game("Before");
    let store =
        savegame_timeline_core::store::Store::open_existing(device.core.repository_root()).unwrap();
    let mut meta = store.catalog().unwrap()[0].meta.clone();
    meta.title = "After".into();
    meta.updated_at += std::time::Duration::from_secs(1);
    store.write_game(&meta).unwrap();

    let Device { core, data, saves } = device;
    drop(core);
    let (core, _) = TimelineCore::open(data.path(), OpenOptions::default()).unwrap();
    assert_eq!(core.get_game(game.game.id).unwrap().game.title, "After");
    drop(saves);
}

#[test]
fn deep_integrity_rebuilds_files_and_flags_a_wrong_size() {
    let device = Device::new();
    device.write("save.dat", "sized");
    device.add_game("Sized");
    let store =
        savegame_timeline_core::store::Store::open_existing(device.core.repository_root()).unwrap();
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
    savegame_timeline_core::store::write_json(&path, &version).unwrap();

    let report = device.core.check_integrity(true).unwrap();
    assert!(!report.is_ok());
    assert_eq!(report.damaged_files.len(), 1);
    assert!(report.corrupt_chunks.is_empty());
}
