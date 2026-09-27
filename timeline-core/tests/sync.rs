//! Two (or three) devices sharing one remote folder. The "machines" share a
//! disk: an adopted game tracks the same folder as on device A, so a
//! device's live saves are set (through `a.write`) right before it acts.

mod common;

use common::Device;
use savegame_timeline_core::maintenance::{self, RetentionSettings};
use savegame_timeline_core::models::GameForm;
use savegame_timeline_core::store::Store;
use savegame_timeline_core::sync::Divergence;
use std::fs;
use std::path::Path;
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
            keep_all_hours: 0,
            hourly_until_hours: 0,
            daily_until_days: 0,
            weekly_until_days: 0,
            branch_expiry_days: None,
            ..RetentionSettings::default()
        })
        .unwrap();
    let report = a.core.run_pruning().unwrap();
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
