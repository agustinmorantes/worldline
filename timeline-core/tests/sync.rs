//! Two (or three) devices sharing one remote folder. The "machines" share a
//! disk: an adopted game tracks the same folder as on device A, so a
//! device's live saves are set (through `a.write`) right before it acts.

mod common;

use common::Device;
use savegame_timeline_core::OpenOptions;
use savegame_timeline_core::TimelineCore;
use savegame_timeline_core::detection::{Notification, NotificationReceiver};
use savegame_timeline_core::maintenance::{self, RetentionSettings};
use savegame_timeline_core::models::GameForm;
use savegame_timeline_core::store::Store;
use savegame_timeline_core::sync::{Divergence, SyncSettings};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

fn uuid_of(game: &savegame_timeline_core::models::FullGame) -> Uuid {
    Uuid::parse_str(&game.game.uuid).unwrap()
}

fn relation(device: &Device, game: Uuid) -> (Divergence, Option<i32>) {
    let report = device.core.refresh_sync().unwrap();
    let status = report.games.iter().find(|s| s.game_uuid == game).unwrap();
    (status.relation, status.remote_version_id)
}

fn remove_version_dir(remote: &Path, version_uuid: &str) -> std::path::PathBuf {
    let short = &Uuid::parse_str(version_uuid).unwrap().simple().to_string()[..8];
    for game in fs::read_dir(remote.join("games")).unwrap() {
        for slot in fs::read_dir(game.unwrap().path().join("slots")).unwrap() {
            for version in fs::read_dir(slot.unwrap().path().join("versions")).unwrap() {
                let path = version.unwrap().path();
                if path.file_name().unwrap().to_string_lossy().ends_with(short) {
                    let parked = remote.join(format!("parked-{short}"));
                    fs::rename(&path, &parked).unwrap();
                    return parked;
                }
            }
        }
    }
    panic!("version dir not found");
}

#[test]
fn adoption_fast_forward_and_divergence() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    a.write("save.dat", "1");
    let game_a = a.add_game("Shared");
    let uuid = uuid_of(&game_a);
    a.core.refresh_sync().unwrap();

    // B adopts with identical live saves: it simply joins the history.
    let b = Device::synced(remote.path());
    let game_b = b.core.adopt_remote_game(uuid).unwrap();
    assert_eq!(game_b.versions.len(), 1);
    assert_eq!(relation(&b, uuid).0, Divergence::InSync);

    // A plays on; B sees a fast-forward it may apply (not auto-applied:
    // the process state of an unknown install is treated as running).
    a.write("save.dat", "2");
    a.checkpoint(game_a.game.id);
    a.core.push_sync().unwrap();
    a.write("save.dat", "1"); // B's machine still holds version 1
    let (ff, target) = relation(&b, uuid);
    assert_eq!(ff, Divergence::FastForward);
    b.core.apply_remote_version(target.unwrap()).unwrap();
    assert_eq!(a.read("save.dat").as_deref(), Some("2"));
    assert_eq!(
        b.game(game_b.game.id).versions.len(),
        2,
        "no extra version when nothing changed"
    );
    assert_eq!(relation(&b, uuid).0, Divergence::InSync);

    // Both play offline: both branches are kept, neither overwritten.
    a.write("save.dat", "A3");
    a.checkpoint(game_a.game.id);
    a.core.push_sync().unwrap();
    a.write("save.dat", "B3");
    b.checkpoint(game_b.game.id);
    assert_eq!(relation(&b, uuid).0, Divergence::Diverged);
    assert_eq!(relation(&a, uuid).0, Divergence::Diverged);
    assert_eq!(b.game(game_b.game.id).versions.len(), 4);
}

#[test]
fn a_child_that_arrives_before_its_parent_waits_without_aborting() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    a.write("save.dat", "1");
    let game = a.add_game("Ordered");
    a.write("save.dat", "2");
    let v2 = a.checkpoint(game.game.id);
    a.write("save.dat", "3");
    a.checkpoint(game.game.id);
    a.core.refresh_sync().unwrap();
    let parked = remove_version_dir(remote.path(), &v2.uuid);

    let b = Device::synced(remote.path());
    fs::remove_file(a.save_dir().join("save.dat")).unwrap(); // B's machine has no saves yet
    let adopted = b.core.adopt_remote_game(uuid_of(&game)).unwrap();
    assert_eq!(
        b.core
            .get_slot(adopted.active_slot.unwrap().id)
            .unwrap()
            .versions
            .len(),
        1
    );

    fs::rename(&parked, parked.parent().unwrap().join("placeholder")).unwrap();
    a.core.push_sync().unwrap(); // A republishes the missing version
    let report = b.core.refresh_sync().unwrap();
    assert!(report.problems.is_empty(), "{:?}", report.problems);
    assert_eq!(b.game(adopted.game.id).versions.len(), 3);
}

#[test]
fn games_tracked_on_both_devices_before_sync_are_linked() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    a.write("save.dat", "a");
    let game = a.add_game("Twice");
    a.core.refresh_sync().unwrap();

    let b = Device::new();
    let local = b.add_game_at("twice", &a.save_dir());
    b.core
        .update_sync_settings(savegame_timeline_core::sync::SyncSettings {
            remote_path: Some(remote.path().to_string_lossy().into_owned()),
        })
        .unwrap();
    let report = b.core.refresh_sync().unwrap();
    assert!(report.problems.is_empty(), "{:?}", report.problems);
    let games = b.core.get_games().unwrap();
    assert_eq!(games.len(), 1);
    assert_eq!(
        games[0].game.uuid, game.game.uuid,
        "the local game took the shared identity"
    );
    assert_eq!(games[0].slots.len(), 2, "both histories are kept as slots");
    assert_eq!(
        games[0].active_slot.as_ref().unwrap().uuid,
        local.active_slot.unwrap().uuid
    );

    // A unrelated local game with the same title is reported, not merged.
    let c = Device::new();
    c.write("save.dat", "c");
    c.add_game("Twice");
    c.core
        .update_sync_settings(savegame_timeline_core::sync::SyncSettings {
            remote_path: Some(remote.path().to_string_lossy().into_owned()),
        })
        .unwrap();
    let report = c.core.refresh_sync().unwrap();
    assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
    assert_eq!(c.core.get_games().unwrap().len(), 1);
}

#[test]
fn renames_and_deletions_propagate_and_are_not_resurrected() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    a.write("save.dat", "x");
    let game = a.add_game("Before");
    let extra = a.core.create_slot(game.game.id, "Extra").unwrap();
    let extra_slot = extra.active_slot.unwrap();
    a.core
        .switch_slot(game.game.id, game.active_slot.as_ref().unwrap().id)
        .unwrap();
    a.core.refresh_sync().unwrap();

    let b = Device::synced(remote.path());
    let adopted = b.core.adopt_remote_game(uuid_of(&game)).unwrap();
    assert_eq!(adopted.slots.len(), 2);

    a.core
        .save_game(
            GameForm {
                id: Some(game.game.id),
                title: "After".into(),
            },
            None,
        )
        .unwrap();
    a.core.delete_slot(extra_slot.id, None).unwrap();
    a.core.refresh_sync().unwrap();
    b.core.refresh_sync().unwrap();
    let on_b = b.game(adopted.game.id);
    assert_eq!(on_b.game.title, "After");
    assert_eq!(on_b.slots.len(), 1);
    a.core.refresh_sync().unwrap();
    assert_eq!(
        a.game(game.game.id).slots.len(),
        1,
        "B's push did not resurrect the slot"
    );

    a.core.delete_game(game.game.id).unwrap();
    a.core.refresh_sync().unwrap();
    b.core.refresh_sync().unwrap();
    assert!(b.core.get_games().unwrap().is_empty());
    assert!(b.core.refresh_sync().unwrap().remote_games.is_empty());
}

#[test]
fn remote_pruning_with_two_devices_keeps_every_protected_version() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    a.write("save.dat", "v1");
    let game = a.add_game("Pruned");
    for n in 2..=5 {
        a.write("save.dat", &format!("v{n}"));
        a.checkpoint(game.game.id);
    }
    a.core.refresh_sync().unwrap();

    // B adopts, then sits on an old version and goes offline.
    let b = Device::synced(remote.path());
    let adopted = b.core.adopt_remote_game(uuid_of(&game)).unwrap();
    let old = adopted
        .versions
        .iter()
        .find(|v| v.version_number == 2)
        .unwrap()
        .clone();
    b.core.restore_version(old.id).unwrap();
    b.core.refresh_sync().unwrap();

    a.write("save.dat", "v5");
    for n in 6..=8 {
        a.write("save.dat", &format!("v{n}"));
        a.checkpoint(game.game.id);
    }
    a.core
        .update_pruning_settings(RetentionSettings {
            automatic: true,
            keep_all_hours: 0,
            hourly_until_hours: 0,
            daily_until_days: 0,
            weekly_until_days: 0,
            branch_expiry_days: None,
            ..RetentionSettings::default()
        })
        .unwrap();
    // An hour later, so this hour's versions are no longer "future" to the planner.
    let report = a
        .core
        .run_pruning_at(chrono::Utc::now() + chrono::Duration::hours(2))
        .unwrap();
    assert!(!report.retired_versions.is_empty());
    assert!(
        !report
            .retired_versions
            .contains(&Uuid::parse_str(&old.uuid).unwrap())
    );
    assert!(report.remote_gc.as_ref().unwrap().chunks_condemned > 0);

    // A later run past the grace period sweeps the remote.
    let store = Store::open_existing(remote.path()).unwrap();
    let before = store.chunk_sizes().unwrap().len();
    let pointed = maintenance_pointers(remote.path());
    let swept = maintenance::gc_remote(
        &store,
        &pointed,
        Uuid::new_v4(),
        chrono::Utc::now() + maintenance::GRACE * 2,
    )
    .unwrap();
    assert!(swept.chunks_deleted > 0);
    assert!(store.chunk_sizes().unwrap().len() < before);
    assert!(
        savegame_timeline_core::integrity::check(&store, true)
            .unwrap()
            .is_ok()
    );

    // Every protected version still restores on both devices.
    b.core.refresh_sync().unwrap();
    a.write("save.dat", "v2"); // B's machine is on version 2
    b.core.restore_version(old.id).unwrap();
    assert_eq!(a.read("save.dat").as_deref(), Some("v2"));
    a.write("save.dat", "v8");
    let on_a = a.game(game.game.id);
    let a_old = on_a.versions.iter().find(|v| v.uuid == old.uuid).unwrap();
    assert!(!a_old.retired);
    a.core.restore_version(a_old.id).unwrap();
    assert_eq!(a.read("save.dat").as_deref(), Some("v2"));
    let root = on_a
        .versions
        .iter()
        .find(|v| v.parent_id.is_none())
        .unwrap();
    a.core.restore_version(root.id).unwrap();
    assert_eq!(a.read("save.dat").as_deref(), Some("v1"));
    assert!(on_a.versions.iter().any(|v| v.retired));
}

#[test]
fn pull_fetches_history_without_touching_saves_or_pointers() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    a.write("save.dat", "v1");
    let game = a.add_game("Pulled");
    a.core.refresh_sync().unwrap();
    let b = Device::synced(remote.path());
    let adopted = b.core.adopt_remote_game(uuid_of(&game)).unwrap();
    let pointer = adopted.slots[0].active_version_id;

    a.write("save.dat", "v2");
    a.checkpoint(game.game.id);
    a.core.push_sync().unwrap();
    a.write("save.dat", "B's own progress");

    assert!(b.core.refresh_sync().unwrap().problems.is_empty());
    let pulled = b.core.get_game(adopted.game.id).unwrap();
    assert_eq!(pulled.versions.len(), 2);
    assert_eq!(pulled.slots[0].active_version_id, pointer);
    assert_eq!(a.read("save.dat").as_deref(), Some("B's own progress"));
}

#[test]
fn a_push_does_not_reupload_versions_another_device_retired() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    a.write("save.dat", "v1");
    let game = a.add_game("Retired elsewhere");
    for n in 2..=5 {
        a.write("save.dat", &format!("v{n}"));
        a.checkpoint(game.game.id);
    }
    a.core.refresh_sync().unwrap();

    let b = Device::synced(remote.path());
    b.core.adopt_remote_game(uuid_of(&game)).unwrap();
    b.core
        .update_pruning_settings(RetentionSettings {
            keep_all_hours: 0,
            hourly_until_hours: 0,
            daily_until_days: 0,
            weekly_until_days: 0,
            branch_expiry_days: None,
            ..RetentionSettings::default()
        })
        .unwrap();
    let report = b
        .core
        .run_pruning_at(chrono::Utc::now() + chrono::Duration::hours(2))
        .unwrap();
    assert!(!report.retired_versions.is_empty());
    let store = Store::open_existing(remote.path()).unwrap();
    maintenance::gc_remote(
        &store,
        &maintenance_pointers(remote.path()),
        Uuid::new_v4(),
        chrono::Utc::now() + maintenance::GRACE * 2,
    )
    .unwrap();
    let swept = store.chunk_sizes().unwrap().len();

    // A hasn't pulled the retirement yet, but its push must respect it.
    a.core.push_sync().unwrap();
    assert_eq!(store.chunk_sizes().unwrap().len(), swept);
}

#[test]
fn an_unreadable_device_state_blocks_cleanup() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    a.write("save.dat", "x");
    a.add_game("Guarded");
    a.core.refresh_sync().unwrap();
    let other = remote
        .path()
        .join("devices")
        .join(Uuid::new_v4().to_string());
    fs::create_dir_all(&other).unwrap();
    fs::write(other.join("state.json"), b"{ half-synced").unwrap();
    let error = a.core.run_pruning().unwrap_err();
    assert!(error.to_string().contains("unreadable"), "{error:#}");
}

fn maintenance_pointers(remote: &Path) -> std::collections::HashSet<Uuid> {
    let mut pointed = std::collections::HashSet::new();
    for device in fs::read_dir(remote.join("devices")).unwrap() {
        let state: savegame_timeline_core::sync::DeviceState =
            savegame_timeline_core::store::read_json(&device.unwrap().path().join("state.json"))
                .unwrap();
        pointed.extend(state.pointed_versions());
    }
    pointed
}

#[test]
fn a_chunk_deleted_from_the_remote_is_reuploaded_by_its_publisher() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    a.write("save.dat", "precious");
    a.add_game("Healed");
    a.core.refresh_sync().unwrap();
    let store = Store::open_existing(remote.path()).unwrap();
    let (hash, _) = store.chunk_sizes().unwrap().into_iter().next().unwrap();
    fs::remove_file(store.chunk_path(&hash).unwrap()).unwrap();

    a.core.push_sync().unwrap();
    assert!(store.has_chunk(&hash));

    // A corrupt remote chunk is set aside on pull and healed the same way.
    fs::write(store.chunk_path(&hash).unwrap(), b"garbage").unwrap();
    let b = Device::synced(remote.path());
    fs::remove_file(a.save_dir().join("save.dat")).unwrap(); // B's machine has no saves yet
    let report = b.core.refresh_sync().unwrap();
    assert!(report.problems.is_empty(), "{:?}", report.problems);
    let game = report.remote_games[0].uuid;
    b.core.adopt_remote_game(game).unwrap();
    assert!(!store.has_chunk(&hash));
    a.core.push_sync().unwrap();
    assert!(
        savegame_timeline_core::integrity::check(&store, true)
            .unwrap()
            .is_ok()
    );
    b.core.refresh_sync().unwrap();
    assert_eq!(b.core.get_games().unwrap()[0].versions.len(), 1);
}

// ---- the background sync loop ----------------------------------------------

/// Opens `data` with the watcher and the sync loop, syncing with `remote`.
fn open_running(data: &Path, remote: Option<&Path>) -> (Arc<TimelineCore>, NotificationReceiver) {
    savegame_timeline_core::sync::save_settings(
        &data.join("sync_settings.json"),
        &SyncSettings {
            remote_path: remote.map(|path| path.to_string_lossy().into_owned()),
        },
    )
    .unwrap();
    let options = OpenOptions {
        watch: true,
        debounce_seconds: None,
    };
    TimelineCore::open_shared(data, options).unwrap()
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn wait_for(rx: &NotificationReceiver, what: &str, want: impl Fn(&Notification) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let left = deadline
            .checked_duration_since(Instant::now())
            .unwrap_or_else(|| panic!("timed out waiting for {what}"));
        match rx.recv_timeout(left) {
            Ok(notification) if want(&notification) => return,
            Ok(_) => {}
            Err(_) => panic!("timed out waiting for {what}"),
        }
    }
}

/// Zero until the background sync has created the remote store.
fn remote_version_count(remote: &Path) -> usize {
    let Ok(store) = Store::open_existing(remote) else {
        return 0;
    };
    store
        .catalog()
        .unwrap()
        .iter()
        .map(|game| game.versions().count())
        .sum()
}

/// A running device with one game whose first version is already published.
fn running_with_game(remote: &Path) -> (Device, Arc<TimelineCore>, NotificationReceiver, i32) {
    let device = Device::new();
    device.write("save.dat", "1");
    let (core, rx) = open_running(device.data.path(), Some(remote));
    fs::create_dir_all(device.save_dir()).unwrap();
    let game = core
        .save_game(
            GameForm {
                id: None,
                title: "Running".into(),
            },
            Some(vec![device.save_dir().to_string_lossy().into_owned()]),
        )
        .unwrap();
    wait_until("the first version on the remote", || {
        remote_version_count(remote) == 1
    });
    (device, core, rx, game.id)
}

#[test]
fn a_manual_checkpoint_reaches_the_remote_without_a_sync_call() {
    let remote = tempfile::tempdir().unwrap();
    let (device, core, _rx, game) = running_with_game(remote.path());
    device.write("save.dat", "2");
    core.checkpoint(game).unwrap();
    wait_until("the manual checkpoint on the remote", || {
        remote_version_count(remote.path()) == 2
    });
    core.shutdown();
}

#[test]
fn a_version_created_just_before_shutdown_is_on_the_remote_afterwards() {
    let remote = tempfile::tempdir().unwrap();
    let (device, core, _rx, game) = running_with_game(remote.path());
    device.write("save.dat", "2");
    core.checkpoint(game).unwrap();
    core.shutdown();
    assert_eq!(remote_version_count(remote.path()), 2);
}

#[test]
fn opening_with_the_loop_picks_up_another_devices_progress() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    a.write("save.dat", "1");
    let game = a.add_game("Shared");
    a.core.refresh_sync().unwrap();
    let b = Device::synced(remote.path());
    b.core.adopt_remote_game(uuid_of(&game)).unwrap();
    drop(b.core);

    a.write("save.dat", "2");
    a.checkpoint(game.game.id);
    a.core.push_sync().unwrap();

    let (core, rx) = open_running(b.data.path(), Some(remote.path()));
    wait_for(&rx, "the start-up sync", |n| {
        matches!(n, Notification::SyncCompleted(_))
    });
    let id = core.get_games().unwrap()[0].game.id;
    assert_eq!(core.get_game(id).unwrap().versions.len(), 2);
    core.shutdown();
}

#[test]
fn the_loop_stops_on_shutdown_and_its_notifications_end() {
    let remote = tempfile::tempdir().unwrap();
    let (_device, core, rx, _game) = running_with_game(remote.path());
    core.shutdown();
    // Every sender is gone once the watcher and the loop have exited.
    while rx.recv_timeout(Duration::from_secs(20)).is_ok() {}
    assert!(matches!(
        rx.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Disconnected)
    ));
}

#[test]
fn an_unreachable_remote_is_reported_and_capture_still_works() {
    let remote = tempfile::tempdir().unwrap();
    let missing = remote.path().join("unplugged");
    let device = Device::new();
    device.write("save.dat", "1");
    let (core, rx) = open_running(device.data.path(), Some(&missing));
    wait_for(&rx, "the failure report", |n| {
        matches!(n, Notification::SyncFailed(_))
    });
    fs::create_dir_all(device.save_dir()).unwrap();
    let game = core
        .save_game(
            GameForm {
                id: None,
                title: "Offline".into(),
            },
            Some(vec![device.save_dir().to_string_lossy().into_owned()]),
        )
        .unwrap();
    device.write("save.dat", "2");
    core.checkpoint(game.id).unwrap();
    assert_eq!(core.get_game(game.id).unwrap().versions.len(), 2);
    core.shutdown();
}

#[test]
fn pruning_runs_on_a_core_with_no_remote() {
    let device = Device::new();
    device.write("save.dat", "1");
    let game = device.add_game("Pruned");
    for n in 2..=4 {
        device.write("save.dat", &n.to_string());
        device.checkpoint(game.game.id);
    }
    device
        .core
        .update_pruning_settings(RetentionSettings {
            automatic: true,
            keep_all_hours: 0,
            hourly_until_hours: 0,
            daily_until_days: 0,
            weekly_until_days: 0,
            ..device.core.get_pruning_settings().unwrap()
        })
        .unwrap();
    // Age every version past the planner's current hour.
    for entry in walk(&device.data.path().join("repository")) {
        if entry.file_name().is_some_and(|n| n == "metadata.json")
            && entry.to_string_lossy().contains("versions")
        {
            let mut meta: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(&entry).unwrap()).unwrap();
            meta["created_at"] = "2020-01-01T00:00:00Z".into();
            meta["updated_at"] = "2020-01-01T00:00:00Z".into();
            fs::write(&entry, serde_json::to_string(&meta).unwrap()).unwrap();
        }
    }
    device.core.reindex().unwrap();
    let Device { core, data, .. } = device;
    drop(core);
    let (core, rx) = open_running(data.path(), None);
    wait_for(&rx, "automatic pruning", |n| {
        matches!(n, Notification::HistoryPruned)
    });
    core.shutdown();
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            found.extend(walk(&path));
        } else {
            found.push(path);
        }
    }
    found
}

#[test]
fn budget_planning_counts_chunks_that_only_the_remote_holds() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    a.write("save.dat", "precious");
    a.add_game("Sized");
    a.core.push_sync().unwrap();
    let local = Store::open_existing(a.core.repository_root()).unwrap();
    let (hash, _) = local.chunk_sizes().unwrap().into_iter().next().unwrap();
    let remote_size = Store::open_existing(remote.path())
        .unwrap()
        .chunk_sizes()
        .unwrap()[&hash];
    fs::remove_file(local.chunk_path(&hash).unwrap()).unwrap();

    let preview = a.core.preview_pruning().unwrap();
    assert_eq!(preview.live_bytes_before, remote_size);
}

#[test]
fn pulled_versions_are_remembered_for_push_healing_and_old_entries_forgotten() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    a.write("save.dat", "old progress");
    let game = a.add_game("Pulled");
    let version = a.active_version(game.game.id);
    a.core.refresh_sync().unwrap();
    fs::remove_file(a.save_dir().join("save.dat")).unwrap(); // B's machine has no saves yet

    let b = Device::synced(remote.path());
    let stale = Uuid::new_v4();
    fs::write(
        b.data.path().join("sync_state.json"),
        format!(r#"{{"recent_pulls": {{"{stale}": "2020-01-01T00:00:00Z"}}}}"#),
    )
    .unwrap();
    let uuid = b.core.refresh_sync().unwrap().remote_games[0].uuid;
    b.core.adopt_remote_game(uuid).unwrap();

    let state: serde_json::Value =
        serde_json::from_slice(&fs::read(b.data.path().join("sync_state.json")).unwrap()).unwrap();
    let pulls = state["recent_pulls"].as_object().unwrap();
    assert!(pulls.contains_key(&version.uuid), "{state}");
    assert!(!pulls.contains_key(&stale.to_string()), "{state}");
}
