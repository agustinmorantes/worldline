//! Deleted games and slots stay recoverable for 30 days (ADR 0011).

mod common;

use chrono::{Duration, Utc};
use common::Device;
use std::collections::HashMap;
use std::fs;
use uuid::Uuid;
use worldline_core::error::{ErrorKind, error_kind};
use worldline_core::maintenance::{GRACE, RetentionSettings};
use worldline_core::models::{DeletedKind, FullGame};
use worldline_core::store::{RECOVERY_WINDOW, Store};
use worldline_core::{OpenOptions, WorldlineCore};

fn uuid_of(game: &FullGame) -> Uuid {
    Uuid::parse_str(&game.game.uuid).unwrap()
}

fn store_of(device: &Device) -> Store {
    Store::open_existing(device.core.repository_root()).unwrap()
}

fn deleted(device: &Device) -> Vec<(DeletedKind, String)> {
    device
        .core
        .recently_deleted()
        .unwrap()
        .into_iter()
        .map(|item| (item.kind, item.title))
        .collect()
}

#[test]
fn a_recovered_game_continues_its_history_without_touching_live_saves() {
    let device = Device::new();
    device.write("save.dat", "v1");
    let game = device.add_game("Kept");
    device.write("save.dat", "v2");
    let v2 = device.checkpoint(game.game.id);
    device.core.delete_game(game.game.id).unwrap();
    assert!(device.core.get_games().unwrap().is_empty());
    assert_eq!(device.read("save.dat").as_deref(), Some("v2"));

    let items = device.core.recently_deleted().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].kind, DeletedKind::Game);
    assert_eq!(items[0].uuid, uuid_of(&game));
    assert_eq!(items[0].expires_at, items[0].deleted_at + RECOVERY_WINDOW);

    // Played on while it was deleted: recovery leaves that on disk.
    device.write("save.dat", "later");
    device.core.recover_game(uuid_of(&game)).unwrap();
    assert_eq!(device.read("save.dat").as_deref(), Some("later"));
    assert!(deleted(&device).is_empty());
    let back = device.game_by_title("Kept").unwrap();
    assert_eq!(back.versions.len(), 2);
    assert_eq!(device.active_version(back.game.id).uuid, v2.uuid);
    // The next checkpoint records the disk on top of where it left off.
    let next = device.checkpoint(back.game.id);
    let parent = device.core.get_version(next.parent_id.unwrap()).unwrap();
    assert_eq!(parent.version.uuid, v2.uuid);
    device.core.reindex().unwrap();
    assert_eq!(device.core.get_games().unwrap().len(), 1);
    assert!(device.core.check_integrity(true).unwrap().is_ok());
}

#[test]
fn delete_recover_and_delete_again_converge_on_two_devices() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    let b = Device::synced(remote.path());
    a.write("save.dat", "v1");
    let game = a.add_game("Shared");
    a.core.refresh_sync().unwrap();
    b.core.adopt_remote_game(uuid_of(&game)).unwrap();

    a.core.delete_game(game.game.id).unwrap();
    a.core.refresh_sync().unwrap();
    b.core.refresh_sync().unwrap();
    assert!(b.core.get_games().unwrap().is_empty());
    assert_eq!(deleted(&b), [(DeletedKind::Game, "Shared".to_string())]);

    b.core.recover_game(uuid_of(&game)).unwrap();
    b.core.refresh_sync().unwrap();
    a.core.refresh_sync().unwrap();
    let on_a = a.game_by_title("Shared").expect("live again on A");
    assert!(b.game_by_title("Shared").is_some());
    // A continues its own line rather than applying anything.
    assert_eq!(a.active_version(on_a.game.id).version_number, 1);
    assert!(deleted(&a).is_empty());

    a.core.delete_game(on_a.game.id).unwrap();
    a.core.refresh_sync().unwrap();
    b.core.refresh_sync().unwrap();
    a.core.refresh_sync().unwrap();
    assert!(a.core.get_games().unwrap().is_empty());
    assert!(b.core.get_games().unwrap().is_empty());
    assert_eq!(deleted(&a).len(), 1);
    assert_eq!(deleted(&b).len(), 1);
}

#[test]
fn a_purge_wins_over_a_concurrent_recovery() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    let b = Device::synced(remote.path());
    a.write("save.dat", "v1");
    let game = a.add_game("Gone");
    a.core.refresh_sync().unwrap();
    b.core.adopt_remote_game(uuid_of(&game)).unwrap();
    a.core.delete_game(game.game.id).unwrap();
    a.core.refresh_sync().unwrap();
    b.core.refresh_sync().unwrap();

    // Both offline: A purges while B recovers.
    a.core.purge_deleted(uuid_of(&game)).unwrap();
    b.core.recover_game(uuid_of(&game)).unwrap();
    assert!(b.game_by_title("Gone").is_some());
    a.core.refresh_sync().unwrap();
    b.core.refresh_sync().unwrap();
    a.core.refresh_sync().unwrap();
    for device in [&a, &b] {
        assert!(device.core.get_games().unwrap().is_empty());
        assert!(deleted(device).is_empty());
        // The metadata stays, so every device learns of the purge.
        assert!(
            store_of(device)
                .find_game(uuid_of(&game))
                .unwrap()
                .is_some()
        );
    }
    assert!(b.core.recover_game(uuid_of(&game)).is_err());
}

#[test]
fn recovery_is_refused_while_another_game_has_its_folder_or_title() {
    let device = Device::new();
    device.write("save.dat", "x");
    let first = device.add_game("First");
    device.core.delete_game(first.game.id).unwrap();
    let second = device.add_game("Second");
    let error = device.core.recover_game(uuid_of(&first)).unwrap_err();
    assert_eq!(error_kind(&error), Some(ErrorKind::FolderTaken));

    device.core.delete_game(second.game.id).unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    device.add_game_at("First", elsewhere.path());
    let error = device.core.recover_game(uuid_of(&first)).unwrap_err();
    assert_eq!(error_kind(&error), Some(ErrorKind::NameTaken));
    assert_eq!(deleted(&device).len(), 2, "nothing was recovered");
}

#[test]
fn a_game_recovered_elsewhere_stays_in_the_cloud_list_when_it_conflicts_here() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    let b = Device::synced(remote.path());
    a.write("save.dat", "v1");
    let game = a.add_game("Clash");
    a.core.refresh_sync().unwrap();
    b.core.adopt_remote_game(uuid_of(&game)).unwrap();
    a.core.delete_game(game.game.id).unwrap();
    a.core.refresh_sync().unwrap();
    b.core.refresh_sync().unwrap();
    // B tracks the folder as a new game, then A recovers the old one.
    let mine = b.add_game_at("Mine", &a.save_dir());
    a.core.recover_game(uuid_of(&game)).unwrap();
    a.core.refresh_sync().unwrap();
    let report = b.core.refresh_sync().unwrap();

    let games = b.core.get_games().unwrap();
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].game.id, mine.game.id);
    assert!(
        report
            .remote_games
            .iter()
            .any(|g| g.uuid == uuid_of(&game) && !g.adopted)
    );
    let error = b.core.adopt_remote_game(uuid_of(&game)).unwrap_err();
    assert_eq!(error_kind(&error), Some(ErrorKind::FolderTaken));
}

#[test]
fn slots_recover_on_their_own_with_a_free_name() {
    let device = Device::new();
    device.write("save.dat", "main");
    let game = device.add_game("Slots");
    let id = game.game.id;
    let default = game.active_slot.clone().unwrap();
    let second = device
        .core
        .create_slot(id, "Second")
        .unwrap()
        .active_slot
        .unwrap();
    device.write("save.dat", "second");
    device.checkpoint(id);
    device
        .core
        .delete_slot(second.id, Some(default.id))
        .unwrap();
    assert_eq!(device.read("save.dat").as_deref(), Some("main"));
    assert_eq!(
        deleted(&device),
        [(DeletedKind::Slot, "Second".to_string())]
    );

    // Recovering the game leaves a slot deleted on its own deleted.
    device.core.delete_game(id).unwrap();
    device.core.recover_game(uuid_of(&game)).unwrap();
    let game = device.game_by_title("Slots").unwrap();
    assert_eq!(game.slots.len(), 1);
    assert_eq!(
        deleted(&device),
        [(DeletedKind::Slot, "Second".to_string())]
    );

    let id = game.game.id;
    device.core.create_slot(id, "Second").unwrap();
    let active = device.game(id).active_slot.unwrap();
    device
        .core
        .recover_slot(Uuid::parse_str(&second.uuid).unwrap())
        .unwrap();
    let game = device.game(id);
    let mut names: Vec<_> = game.slots.iter().map(|slot| slot.name.as_str()).collect();
    names.sort();
    assert_eq!(names, ["Default Slot", "Second", "Second (2)"]);
    assert_eq!(
        game.active_slot.unwrap().id,
        active.id,
        "it comes back inactive"
    );
    let recovered = game.slots.iter().find(|s| s.name == "Second (2)").unwrap();
    assert_eq!(
        device.core.get_slot(recovered.id).unwrap().versions.len(),
        1
    );
    assert!(deleted(&device).is_empty());
}

#[test]
fn a_slot_deleted_while_another_device_played_offline_still_recovers() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    let b = Device::synced(remote.path());
    a.write("save.dat", "main");
    let game = a.add_game("Offline");
    let default = game.active_slot.clone().unwrap();
    let second = a
        .core
        .create_slot(game.game.id, "Second")
        .unwrap()
        .active_slot
        .unwrap();
    a.write("save.dat", "second");
    a.checkpoint(game.game.id);
    a.core.refresh_sync().unwrap();
    let id = b.core.adopt_remote_game(uuid_of(&game)).unwrap().game.id;
    // B plays offline in the slot A then deletes.
    a.write("save.dat", "offline");
    b.checkpoint(id);
    a.core.delete_slot(second.id, Some(default.id)).unwrap();
    a.core.refresh_sync().unwrap();
    b.core.refresh_sync().unwrap();
    let kept = b.game(id).active_slot.unwrap();
    assert_eq!(kept.name, "Second");
    assert_ne!(
        kept.uuid, second.uuid,
        "unseen progress moved to a new slot"
    );

    b.core
        .recover_slot(Uuid::parse_str(&second.uuid).unwrap())
        .unwrap();
    let names: Vec<_> = b.game(id).slots.into_iter().map(|s| s.name).collect();
    assert!(names.contains(&"Second (2)".to_string()));
    b.core.refresh_sync().unwrap();
    a.core.refresh_sync().unwrap();
    assert!(
        a.game(game.game.id)
            .slots
            .iter()
            .any(|slot| slot.uuid == second.uuid)
    );
}

fn aggressive() -> RetentionSettings {
    RetentionSettings {
        automatic: true,
        keep_all_hours: 0,
        hourly_until_hours: 0,
        daily_until_days: 0,
        weekly_until_days: 0,
        branch_expiry_days: None,
        ..RetentionSettings::default()
    }
}

/// A synced game with five versions, a screenshot on the oldest, deleted.
fn deleted_with_history(remote: &std::path::Path) -> (Device, FullGame, std::path::PathBuf) {
    let device = Device::synced(remote);
    device.write("save.dat", "v1");
    let game = device.add_game("Collected");
    let first = device.active_version(game.game.id);
    for n in 2..=5 {
        device.write("save.dat", &format!("v{n}"));
        device.checkpoint(game.game.id);
    }
    let version = device.core.get_version(first.id).unwrap();
    let store = store_of(&device);
    store
        .write_screenshot(
            &version.game.folder_slug,
            &version.slot.folder_slug,
            &version.version.folder_slug,
            b"jpeg",
        )
        .unwrap();
    let screenshot = store.screenshot_path(
        &version.game.folder_slug,
        &version.slot.folder_slug,
        &version.version.folder_slug,
    );
    device.core.update_pruning_settings(aggressive()).unwrap();
    device.core.refresh_sync().unwrap();
    device.core.delete_game(game.game.id).unwrap();
    (device, game, screenshot)
}

#[test]
fn recoverable_history_is_frozen_and_kept_until_its_window_ends() {
    let remote = tempfile::tempdir().unwrap();
    let (device, game, screenshot) = deleted_with_history(remote.path());
    let local = store_of(&device);
    let remote_store = Store::open_existing(remote.path()).unwrap();
    let chunks = local.chunk_sizes().unwrap().len();
    let remote_chunks = remote_store.chunk_sizes().unwrap().len();
    assert!(chunks >= 5);

    let now = Utc::now() + Duration::hours(2);
    for at in [now, now + GRACE * 2] {
        let report = device.core.run_pruning_at(at).unwrap();
        assert!(
            report.retired_versions.is_empty(),
            "deleted history is frozen"
        );
    }
    assert_eq!(local.chunk_sizes().unwrap().len(), chunks);
    assert_eq!(remote_store.chunk_sizes().unwrap().len(), remote_chunks);
    assert!(screenshot.exists());

    // Recovered within the window, it is whole.
    device.core.recover_game(uuid_of(&game)).unwrap();
    assert!(device.core.check_integrity(true).unwrap().is_ok());
    let back = device.game_by_title("Collected").unwrap();
    device.core.delete_game(back.game.id).unwrap();

    // Past the window: collected locally at once, remotely after the grace.
    let expired = Utc::now() + RECOVERY_WINDOW + Duration::hours(1);
    device.core.run_pruning_at(expired).unwrap();
    assert!(local.chunk_sizes().unwrap().is_empty());
    assert!(!screenshot.exists());
    device.core.run_pruning_at(expired + GRACE * 2).unwrap();
    assert!(remote_store.chunk_sizes().unwrap().is_empty());
    assert!(local.find_game(uuid_of(&game)).unwrap().is_some());
}

#[test]
fn a_purged_game_is_collected_by_the_next_cleanup() {
    let remote = tempfile::tempdir().unwrap();
    let (device, game, screenshot) = deleted_with_history(remote.path());
    device.core.purge_deleted(uuid_of(&game)).unwrap();
    assert!(device.core.recently_deleted().unwrap().is_empty());
    let now = Utc::now();
    device.core.run_pruning_at(now).unwrap();
    assert!(store_of(&device).chunk_sizes().unwrap().is_empty());
    assert!(!screenshot.exists());
    // The pointer this device published before deleting holds its version
    // until the run after the next push.
    for at in [now + GRACE * 2, now + GRACE * 4] {
        device.core.run_pruning_at(at).unwrap();
    }
    let remote_store = Store::open_existing(remote.path()).unwrap();
    assert!(remote_store.chunk_sizes().unwrap().is_empty());
}

#[test]
fn a_recoverable_game_exports_as_a_live_game() {
    let a = Device::new();
    a.write("save.dat", "v1");
    let game = a.add_game("Exported");
    a.write("save.dat", "v2");
    a.checkpoint(game.game.id);
    a.core.delete_game(game.game.id).unwrap();
    let out = tempfile::tempdir().unwrap();
    let bundle = out.path().join("exported.zip");
    a.core.export_deleted_game(uuid_of(&game), &bundle).unwrap();

    let b = Device::new();
    fs::create_dir_all(b.save_dir()).unwrap();
    let map: HashMap<_, _> = WorldlineCore::inspect_history_bundle(&bundle)
        .unwrap()
        .active_tracked_dirs
        .iter()
        .map(|dir| (dir.uuid, b.save_dir()))
        .collect();
    let report = b.core.import_game_history(&bundle, &map).unwrap();
    assert_eq!(report.version_count, 2);
    assert!(b.game_by_title("Exported").is_some());
    assert!(b.core.check_integrity(true).unwrap().is_ok());

    // On the device that deleted it, importing the bundle brings it back.
    let map: HashMap<_, _> = map.into_keys().map(|uuid| (uuid, a.save_dir())).collect();
    a.core.import_game_history(&bundle, &map).unwrap();
    assert!(a.game_by_title("Exported").is_some());
    assert!(deleted(&a).is_empty());

    // A purged game can't be exported.
    let id = a.game_by_title("Exported").unwrap().game.id;
    a.core.delete_game(id).unwrap();
    a.core.purge_deleted(uuid_of(&game)).unwrap();
    let again = out.path().join("again.zip");
    assert!(a.core.export_deleted_game(uuid_of(&game), &again).is_err());
    assert!(!again.exists());
}

#[test]
fn a_newer_store_format_needs_an_update_and_is_never_moved_aside() {
    let data = tempfile::tempdir().unwrap();
    let marker = data.path().join("repository").join("metadata.json");
    fs::create_dir_all(marker.parent().unwrap()).unwrap();
    let newer = r#"{"format_version":5,"created_at":"2026-01-01T00:00:00Z","app":"worldline"}"#;
    fs::write(&marker, newer).unwrap();
    let error = WorldlineCore::open(data.path(), OpenOptions::default())
        .err()
        .unwrap();
    assert_eq!(error_kind(&error), Some(ErrorKind::UpdateRequired));
    assert_eq!(fs::read_to_string(&marker).unwrap(), newer);

    // A sync folder written by a newer app is refused the same way.
    let remote = tempfile::tempdir().unwrap();
    fs::write(remote.path().join("metadata.json"), newer).unwrap();
    let device = Device::new();
    let error = device
        .core
        .update_sync_settings(worldline_core::sync::SyncSettings {
            remote_path: Some(remote.path().to_string_lossy().into_owned()),
        })
        .and_then(|_| device.core.refresh_sync())
        .unwrap_err();
    assert_eq!(error_kind(&error), Some(ErrorKind::UpdateRequired));
}
