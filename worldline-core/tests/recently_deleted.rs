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
fn a_recovered_game_resumes_the_playthrough_and_version_it_was_on() {
    let device = Device::new();
    device.write("save.dat", "v1");
    let game = device.add_game("Resumed");
    let id = game.game.id;
    let default = game.active_slot.clone().unwrap();
    let v1 = device.active_version(id);
    device.write("save.dat", "v2");
    device.checkpoint(id);
    device.core.restore_version(v1.id).unwrap();
    // The newest version this device made is in another playthrough.
    device.core.create_slot(id, "Second").unwrap();
    device.write("save.dat", "second");
    device.checkpoint(id);
    device.core.switch_slot(id, default.id).unwrap();
    assert_eq!(device.read("save.dat").as_deref(), Some("v1"));

    device.core.delete_game(id).unwrap();
    assert!(device.core.recover_game(uuid_of(&game)).unwrap());
    let back = device.game_by_title("Resumed").unwrap();
    assert_eq!(back.active_slot.unwrap().uuid, default.uuid);
    assert_eq!(device.active_version(back.game.id).uuid, v1.uuid);
    assert_eq!(device.read("save.dat").as_deref(), Some("v1"));
}

#[test]
fn recovering_a_game_this_device_does_not_track_leaves_it_in_the_cloud() {
    let device = Device::new();
    device.write("save.dat", "v1");
    let game = device.add_game("Elsewhere");
    device.core.delete_game(game.game.id).unwrap();
    fs::write(
        device.data.path().join("untracked_games.json"),
        format!("[\"{}\"]", uuid_of(&game)),
    )
    .unwrap();
    device.core.reindex().unwrap();

    assert!(!device.core.recover_game(uuid_of(&game)).unwrap());
    assert!(device.core.get_games().unwrap().is_empty());
    assert!(deleted(&device).is_empty());
    let meta = store_of(&device)
        .find_game(uuid_of(&game))
        .unwrap()
        .unwrap();
    assert!(!meta.deletion.is_deleted());
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

    assert!(b.core.recover_game(uuid_of(&game)).unwrap());
    b.core.refresh_sync().unwrap();
    a.write("save.dat", "played meanwhile");
    let report = a.core.refresh_sync().unwrap();
    assert!(b.game_by_title("Shared").is_some());
    // Recovered elsewhere, it waits in A's cloud list: nothing here is
    // watched or written until A adopts it.
    assert!(a.core.get_games().unwrap().is_empty());
    assert!(
        report
            .remote_games
            .iter()
            .any(|g| g.uuid == uuid_of(&game) && !g.adopted)
    );
    assert!(deleted(&a).is_empty());
    a.core.refresh_sync().unwrap();
    assert!(a.core.get_games().unwrap().is_empty(), "still not adopted");
    assert_eq!(a.read("save.dat").as_deref(), Some("played meanwhile"));
    let on_a = a.core.adopt_remote_game(uuid_of(&game)).unwrap();
    // Adopting keeps what is on disk, on top of the recovered history.
    assert_eq!(a.read("save.dat").as_deref(), Some("played meanwhile"));
    assert_eq!(a.active_version(on_a.game.id).version_number, 2);

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

/// Deleting the current playthrough keeps the progress nobody recorded yet in
/// the deleted playthrough, whether it had history or not, and whether the
/// one it switches to has any. Recovering writes no live saves.
#[test]
fn deleting_the_current_playthrough_keeps_its_unrecorded_progress_to_recover() {
    let device = Device::new();
    device.write("save.dat", "main");
    let game = device.add_game("Current");
    let id = game.game.id;
    let main = game.active_slot.clone().unwrap();
    let fresh = device.core.create_slot(id, "Fresh").unwrap();
    let fresh = fresh.active_slot.unwrap();
    assert!(device.core.get_slot(fresh.id).unwrap().versions.is_empty());

    // No history yet: the progress becomes its root version.
    device.write("save.dat", "fresh, unrecorded");
    let after = device.core.delete_slot(fresh.id, Some(main.id)).unwrap();
    assert_eq!(after.active_slot.unwrap().id, main.id);
    assert_eq!(device.read("save.dat").as_deref(), Some("main"));

    // Into a playthrough with no history: the folder is emptied.
    let blank = device.core.create_slot(id, "Blank").unwrap();
    let blank = blank.active_slot.unwrap();
    device.core.switch_slot(id, main.id).unwrap();
    device.write("save.dat", "main, unrecorded");
    device.core.delete_slot(main.id, Some(blank.id)).unwrap();
    assert_eq!(device.read("save.dat"), None);

    for (slot, name) in [(&fresh, "Fresh"), (&main, "Default Slot")] {
        device
            .core
            .recover_slot(Uuid::parse_str(&slot.uuid).unwrap())
            .unwrap();
        let game = device.game(id);
        assert_eq!(game.active_slot.unwrap().id, blank.id, "back inactive");
        assert_eq!(device.read("save.dat"), None, "no live writes");
        assert!(game.slots.iter().any(|s| s.name == name));
    }
    let slot_of = |name: &str| {
        let game = device.game(id);
        game.slots.into_iter().find(|s| s.name == name).unwrap().id
    };
    device.core.switch_slot(id, slot_of("Fresh")).unwrap();
    assert_eq!(
        device.read("save.dat").as_deref(),
        Some("fresh, unrecorded")
    );
    device
        .core
        .switch_slot(id, slot_of("Default Slot"))
        .unwrap();
    assert_eq!(device.read("save.dat").as_deref(), Some("main, unrecorded"));
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

/// A game with three versions and a screenshot, synced to B, deleted on A and
/// recovered on B within the window. Before A pulls the recovery its own
/// cleanup collects the history at the window's end (its clock runs ahead, or
/// its pulls failed). Returns A, the game, the screenshot and A's chunk count
/// before the cleanup.
fn collected_here_recovered_elsewhere(
    remote: &std::path::Path,
) -> (Device, FullGame, std::path::PathBuf, usize) {
    let a = Device::synced(remote);
    a.write("save.dat", "v1");
    let game = a.add_game("Revived");
    let first = a.active_version(game.game.id);
    for n in 2..=3 {
        a.write("save.dat", &format!("v{n}"));
        a.checkpoint(game.game.id);
    }
    let version = a.core.get_version(first.id).unwrap();
    let store = store_of(&a);
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
    a.core.refresh_sync().unwrap();
    let b = Device::synced(remote);
    b.core.adopt_remote_game(uuid_of(&game)).unwrap();
    a.core.delete_game(game.game.id).unwrap();
    a.core.refresh_sync().unwrap();
    b.core.refresh_sync().unwrap();
    assert!(b.core.recover_game(uuid_of(&game)).unwrap());
    b.core.refresh_sync().unwrap();

    let chunks = store.chunk_sizes().unwrap().len();
    assert!(chunks >= 3);
    a.core
        .run_pruning_at(Utc::now() + RECOVERY_WINDOW + Duration::hours(1))
        .unwrap();
    assert!(store.chunk_sizes().unwrap().is_empty());
    assert!(!screenshot.exists());
    (a, game, screenshot, chunks)
}

#[test]
fn history_collected_here_heals_from_the_remote_once_recovered_elsewhere() {
    let remote = tempfile::tempdir().unwrap();
    let (a, game, screenshot, chunks) = collected_here_recovered_elsewhere(remote.path());
    let uuid = uuid_of(&game);

    let report = a.core.refresh_sync().unwrap();
    assert!(report.problems.is_empty(), "{:?}", report.problems);
    assert_eq!(store_of(&a).chunk_sizes().unwrap().len(), chunks);
    assert!(screenshot.exists());
    assert_eq!(a.read("save.dat").as_deref(), Some("v3"), "no live writes");
    assert!(a.core.get_games().unwrap().is_empty());
    assert!(
        report
            .remote_games
            .iter()
            .any(|g| g.uuid == uuid && !g.adopted)
    );

    let back = a.core.adopt_remote_game(uuid).unwrap();
    assert_eq!(a.read("save.dat").as_deref(), Some("v3"));
    assert_eq!(back.versions.len(), 3);
    assert!(a.core.check_integrity(true).unwrap().is_ok());
    let out = tempfile::tempdir().unwrap();
    let exported = a
        .core
        .export_game_history(back.game.id, &out.path().join("revived.zip"))
        .unwrap();
    assert_eq!(exported.version_count, 3);
}

#[test]
fn history_missing_everywhere_is_reported_until_it_arrives() {
    let remote = tempfile::tempdir().unwrap();
    let (a, game, _, chunks) = collected_here_recovered_elsewhere(remote.path());
    let held = remote.path().join("chunks");
    let aside = remote.path().join("chunks-aside");
    fs::rename(&held, &aside).unwrap();

    for _ in 0..2 {
        let report = a.core.refresh_sync().unwrap();
        assert!(
            report
                .problems
                .iter()
                .any(|p| p.starts_with("Revived:") && p.contains("not fully arrived")),
            "{:?}",
            report.problems
        );
        assert!(a.core.get_games().unwrap().is_empty());
    }

    if held.exists() {
        fs::remove_dir(&held).unwrap();
    }
    fs::rename(&aside, &held).unwrap();
    let report = a.core.refresh_sync().unwrap();
    assert!(report.problems.is_empty(), "{:?}", report.problems);
    assert_eq!(store_of(&a).chunk_sizes().unwrap().len(), chunks);
    a.core.adopt_remote_game(uuid_of(&game)).unwrap();
    assert!(a.core.check_integrity(true).unwrap().is_ok());
}

#[test]
fn versions_reach_a_game_recovered_here_after_the_remote_window_ended() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    let b = Device::synced(remote.path());
    a.write("save.dat", "v1");
    let game = a.add_game("Late");
    let uuid = uuid_of(&game);
    a.core.refresh_sync().unwrap();
    let on_b = b.core.adopt_remote_game(uuid).unwrap();
    // Both "machines" share one disk in tests: B plays in A's folder.
    a.write("save.dat", "from b");
    b.checkpoint(on_b.game.id);
    b.core.refresh_sync().unwrap();

    // The remote says it was deleted 31 days ago; A, offline, had recovered
    // it on day 29. B's version is still in the sync folder.
    let deleted_at = Utc::now() - RECOVERY_WINDOW - Duration::days(1);
    let remote_store = Store::open_existing(remote.path()).unwrap();
    let mut meta = remote_store.find_game(uuid).unwrap().unwrap();
    meta.deletion.deleted_at = Some(deleted_at);
    remote_store.write_game(&meta).unwrap();
    let local = store_of(&a);
    let mut meta = local.find_game(uuid).unwrap().unwrap();
    meta.deletion.deleted_at = Some(deleted_at);
    meta.deletion.recovered_at = Some(deleted_at + Duration::days(29));
    local.write_game(&meta).unwrap();

    a.core.refresh_sync().unwrap();
    assert_eq!(a.game(game.game.id).versions.len(), 2);
    assert!(a.core.check_integrity(true).unwrap().is_ok());
}

fn reopen(device: Device) -> Device {
    let Device { core, data, saves } = device;
    drop(core);
    let (core, _) = WorldlineCore::open(data.path(), OpenOptions::default()).unwrap();
    Device { core, data, saves }
}

fn pointers_file(device: &Device) -> std::path::PathBuf {
    device.data.path().join("deleted_pointers.json")
}

/// A game whose first playthrough is on its older version while a newer
/// version waits in a second one. Returns the game and that slot and version.
fn on_an_older_version(device: &Device, title: &str) -> (FullGame, String, String) {
    device.write("save.dat", "v1");
    let game = device.add_game(title);
    let id = game.game.id;
    let default = game.active_slot.clone().unwrap();
    let v1 = device.active_version(id);
    device.write("save.dat", "v2");
    device.checkpoint(id);
    device.core.restore_version(v1.id).unwrap();
    device.core.create_slot(id, "Second").unwrap();
    device.write("save.dat", "second");
    device.checkpoint(id);
    device.core.switch_slot(id, default.id).unwrap();
    assert_eq!(device.read("save.dat").as_deref(), Some("v1"));
    (game, default.uuid, v1.uuid)
}

fn resumes(device: &Device, title: &str, slot: &str, version: &str) {
    let back = device.game_by_title(title).unwrap();
    assert_eq!(back.active_slot.unwrap().uuid, slot);
    assert_eq!(device.active_version(back.game.id).uuid, version);
    assert_eq!(
        device.read("save.dat").as_deref(),
        Some("v1"),
        "no live writes"
    );
}

#[test]
fn a_damaged_pointer_file_never_blocks_deleting_or_restoring() {
    let device = Device::new();
    let (game, slot, version) = on_an_older_version(&device, "Damaged");
    let uuid = uuid_of(&game);
    fs::write(pointers_file(&device), "{ not json").unwrap();

    device.core.delete_game(game.game.id).unwrap();
    let aside = fs::read_dir(device.data.path())
        .unwrap()
        .filter_map(|entry| entry.unwrap().file_name().into_string().ok())
        .filter(|name| name.starts_with(".deleted_pointers.corrupt-"))
        .count();
    assert_eq!(aside, 1, "set aside, not lost");
    assert!(device.core.recover_game(uuid).unwrap());
    resumes(&device, "Damaged", &slot, &version);

    // Damaged after the deletion: it still comes back, without live writes.
    let id = device.game_by_title("Damaged").unwrap().game.id;
    device.core.delete_game(id).unwrap();
    fs::write(pointers_file(&device), "garbage").unwrap();
    assert!(device.core.recover_game(uuid).unwrap());
    let back = device.game_by_title("Damaged").unwrap();
    assert!(back.last_version.is_some());
    assert_eq!(
        device.read("save.dat").as_deref(),
        Some("v1"),
        "no live writes"
    );
}

#[test]
fn a_damaged_pointer_file_never_blocks_a_deletion_from_another_device() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    let (game, slot, version) = on_an_older_version(&a, "Elsewhere");
    let uuid = uuid_of(&game);
    a.core.refresh_sync().unwrap();
    // Another device publishes the deletion and its changed stamp.
    let b = Device::synced(remote.path());
    let on_b = b.core.adopt_remote_game(uuid).unwrap();
    b.core.delete_game(on_b.game.id).unwrap();
    b.core.push_sync().unwrap();
    fs::write(pointers_file(&a), "garbage").unwrap();

    let report = a.core.refresh_sync().unwrap();
    assert!(report.problems.is_empty(), "{:?}", report.problems);
    assert!(a.game_by_title("Elsewhere").is_none());
    let a = reopen(a);
    assert!(a.core.recover_game(uuid).unwrap());
    resumes(&a, "Elsewhere", &slot, &version);
}

#[test]
fn pointers_kept_before_a_deletion_survive_a_crash_before_the_library_caught_up() {
    let device = Device::new();
    let (game, slot, version) = on_an_older_version(&device, "Crashed");
    let uuid = uuid_of(&game);
    // A damaged untracked list stops the deletion right after it is
    // written, as a crash there would.
    let untracked = device.data.path().join("untracked_games.json");
    fs::write(&untracked, "garbage").unwrap();
    assert!(device.core.delete_game(game.game.id).is_err());
    let meta = store_of(&device).find_game(uuid).unwrap().unwrap();
    assert!(meta.deletion.is_deleted());
    fs::remove_file(&untracked).unwrap();

    let device = reopen(device);
    assert!(device.game_by_title("Crashed").is_none());
    assert!(device.core.recover_game(uuid).unwrap());
    resumes(&device, "Crashed", &slot, &version);
}

/// Moves every version's creation `days` back, as if made that long ago.
fn backdate_versions(root: &std::path::Path, days: i64) {
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            backdate_versions(&path, days);
            continue;
        }
        let in_versions = path
            .parent()
            .and_then(|dir| dir.parent())
            .and_then(|dir| dir.file_name())
            .is_some_and(|name| name == "versions");
        if !in_versions || path.file_name().unwrap() != "metadata.json" {
            continue;
        }
        let mut meta: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let at =
            chrono::DateTime::parse_from_rfc3339(meta["created_at"].as_str().unwrap()).unwrap();
        meta["created_at"] = serde_json::json!((at - Duration::days(days)).to_rfc3339());
        fs::write(&path, serde_json::to_vec_pretty(&meta).unwrap()).unwrap();
    }
}

fn remote_chunks(remote: &std::path::Path) -> usize {
    Store::open_existing(remote)
        .unwrap()
        .chunk_sizes()
        .unwrap()
        .len()
}

fn uploading(device: &Device) -> Vec<String> {
    let state: serde_json::Value =
        serde_json::from_slice(&fs::read(device.data.path().join("sync_state.json")).unwrap())
            .unwrap();
    state["uploading"]
        .as_array()
        .map(|uuids| {
            uuids
                .iter()
                .map(|u| u.as_str().unwrap().to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// Another device, keeping every version, cleans the sync folder up twice,
/// the second time past the grace period.
fn collect_remote(remote: &std::path::Path) {
    let b = Device::synced(remote);
    b.core
        .update_pruning_settings(RetentionSettings {
            keep_all_hours: 24 * 3650,
            hourly_until_hours: 24 * 3650,
            daily_until_days: 3650,
            weekly_until_days: 3650,
            ..RetentionSettings::default()
        })
        .unwrap();
    b.core.run_pruning_at(Utc::now()).unwrap();
    b.core
        .run_pruning_at(Utc::now() + GRACE + Duration::hours(1))
        .unwrap();
}

/// A synced game with three versions older than the push heal window and a
/// screenshot on the first. A deletes it offline while the sync folder, told
/// another device deleted it 31 days ago, has its history collected. Returns
/// A, the game, the screenshot's path in the sync folder and how many chunks
/// the folder had.
fn collected_in_the_cloud_deleted_here(
    remote: &std::path::Path,
) -> (Device, FullGame, std::path::PathBuf, usize) {
    let a = Device::synced(remote);
    a.write("save.dat", "v1");
    let game = a.add_game("Uploaded");
    let uuid = uuid_of(&game);
    let first = a.active_version(game.game.id);
    for n in 2..=3 {
        a.write("save.dat", &format!("v{n}"));
        a.checkpoint(game.game.id);
    }
    let version = a.core.get_version(first.id).unwrap();
    store_of(&a)
        .write_screenshot(
            &version.game.folder_slug,
            &version.slot.folder_slug,
            &version.version.folder_slug,
            b"jpeg",
        )
        .unwrap();
    // Set fixture timestamps before publishing immutable version metadata.
    backdate_versions(a.core.repository_root(), 10);
    a.core.refresh_sync().unwrap();
    let remote_store = Store::open_existing(remote).unwrap();
    let screenshot = remote_store.screenshot_path(
        &version.game.folder_slug,
        &version.slot.folder_slug,
        &version.version.folder_slug,
    );
    assert!(screenshot.exists());
    let chunks = remote_chunks(remote);

    a.core.delete_game(game.game.id).unwrap();
    let mut meta = remote_store.find_game(uuid).unwrap().unwrap();
    meta.deletion.deleted_at = Some(Utc::now() - RECOVERY_WINDOW - Duration::days(1));
    remote_store.write_game(&meta).unwrap();
    collect_remote(remote);
    assert!(remote_chunks(remote) < chunks);
    assert!(!screenshot.exists());
    (a, game, screenshot, chunks)
}

#[test]
fn history_collected_in_the_cloud_is_uploaded_again_once_recovered_here() {
    let remote = tempfile::tempdir().unwrap();
    let (a, game, screenshot, chunks) = collected_in_the_cloud_deleted_here(remote.path());
    let uuid = uuid_of(&game);

    assert!(a.core.recover_game(uuid).unwrap());
    assert_eq!(uploading(&a), [uuid.to_string()]);
    let report = a.core.refresh_sync().unwrap();
    assert!(report.problems.is_empty(), "{:?}", report.problems);
    assert_eq!(remote_chunks(remote.path()), chunks);
    assert!(screenshot.exists());
    assert!(uploading(&a).is_empty(), "only until one push completes");
    assert_eq!(a.read("save.dat").as_deref(), Some("v3"), "no live writes");

    let c = Device::synced(remote.path());
    c.core.refresh_sync().unwrap();
    let on_c = c.core.adopt_remote_game(uuid).unwrap();
    assert_eq!(on_c.versions.len(), 3);
    assert!(c.core.check_integrity(true).unwrap().is_ok());
    let out = tempfile::tempdir().unwrap();
    let exported = c
        .core
        .export_game_history(on_c.game.id, &out.path().join("uploaded.zip"))
        .unwrap();
    assert_eq!(exported.version_count, 3);
}

#[test]
fn a_failed_upload_of_recovered_history_is_retried_after_a_restart() {
    let remote = tempfile::tempdir().unwrap();
    let (a, game, screenshot, chunks) = collected_in_the_cloud_deleted_here(remote.path());
    let uuid = uuid_of(&game);
    assert!(a.core.recover_game(uuid).unwrap());

    // The sync folder can't take chunks for now.
    let held = remote.path().join("chunks");
    let aside = remote.path().join("chunks-aside");
    fs::rename(&held, &aside).unwrap();
    fs::write(&held, "not a folder").unwrap();
    let _ = a.core.push_sync();
    assert_eq!(uploading(&a), [uuid.to_string()]);
    fs::remove_file(&held).unwrap();
    fs::rename(&aside, &held).unwrap();

    let a = reopen(a);
    assert_eq!(uploading(&a), [uuid.to_string()]);
    a.core.push_sync().unwrap();
    assert_eq!(remote_chunks(remote.path()), chunks);
    assert!(screenshot.exists());
    assert!(uploading(&a).is_empty());
}

#[test]
fn a_playthrough_recovered_here_uploads_its_collected_history_again() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    a.write("save.dat", "v1");
    let game = a.add_game("Playthroughs");
    let uuid = uuid_of(&game);
    let id = game.game.id;
    let default = game.active_slot.clone().unwrap();
    a.core.create_slot(id, "Second").unwrap();
    for n in 1..=3 {
        a.write("save.dat", &format!("s{n}"));
        a.checkpoint(id);
    }
    let second = a.game(id).active_slot.unwrap();
    a.core.switch_slot(id, default.id).unwrap();
    // Set fixture timestamps before publishing immutable version metadata.
    backdate_versions(a.core.repository_root(), 10);
    a.core.refresh_sync().unwrap();
    let chunks = remote_chunks(remote.path());

    a.core.delete_slot(second.id, None).unwrap();
    let remote_store = Store::open_existing(remote.path()).unwrap();
    let record = remote_store.game(uuid).unwrap().unwrap();
    let mut meta = record
        .slots
        .iter()
        .find(|slot| slot.meta.uuid.to_string() == second.uuid)
        .unwrap()
        .meta
        .clone();
    meta.deletion.deleted_at = Some(Utc::now() - RECOVERY_WINDOW - Duration::days(1));
    remote_store
        .write_slot(&record.meta.folder_slug, &meta)
        .unwrap();
    collect_remote(remote.path());
    assert!(remote_chunks(remote.path()) < chunks);

    a.core
        .recover_slot(Uuid::parse_str(&second.uuid).unwrap())
        .unwrap();
    let report = a.core.refresh_sync().unwrap();
    assert!(report.problems.is_empty(), "{:?}", report.problems);
    assert_eq!(remote_chunks(remote.path()), chunks);
    assert!(uploading(&a).is_empty());
    assert_eq!(a.read("save.dat").as_deref(), Some("v1"), "no live writes");
}
