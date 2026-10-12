//! Two (or three) devices sharing one remote folder. The "machines" share a
//! disk: an adopted game tracks the same folder as on device A, so a
//! device's live saves are set (through `a.write`) right before it acts.

mod common;

use common::Device;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;
use worldline_core::OpenOptions;
use worldline_core::WorldlineCore;
use worldline_core::detection::{Notification, NotificationReceiver};
use worldline_core::error::{ErrorKind, error_kind};
use worldline_core::maintenance::{self, RetentionSettings};
use worldline_core::models::{GameForm, Slot};
use worldline_core::store::Store;
use worldline_core::sync::{Divergence, SyncSettings};

fn uuid_of(game: &worldline_core::models::FullGame) -> Uuid {
    Uuid::parse_str(&game.game.uuid).unwrap()
}

fn relation(device: &Device, game: Uuid) -> (Divergence, Option<i32>) {
    let report = device.core.refresh_sync().unwrap();
    let status = report.games.iter().find(|s| s.game_uuid == game).unwrap();
    (status.relation, status.remote_version_id)
}

/// Moves a version's folder out of the remote; returns where it went and
/// where it was.
fn remove_version_dir(remote: &Path, version_uuid: &str) -> (PathBuf, PathBuf) {
    let short = &Uuid::parse_str(version_uuid).unwrap().simple().to_string()[..8];
    for game in fs::read_dir(remote.join("games")).unwrap() {
        for slot in fs::read_dir(game.unwrap().path().join("slots")).unwrap() {
            for version in fs::read_dir(slot.unwrap().path().join("versions")).unwrap() {
                let path = version.unwrap().path();
                if path.file_name().unwrap().to_string_lossy().ends_with(short) {
                    let parked = remote.join(format!("parked-{short}"));
                    fs::rename(&path, &parked).unwrap();
                    return (parked, path);
                }
            }
        }
    }
    panic!("version dir not found");
}

#[test]
fn published_progress_is_in_sync_without_other_devices_to_compare() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    a.write("save.dat", "1");
    let game_a = a.add_game("Alone");
    let uuid = uuid_of(&game_a);
    assert_eq!(relation(&a, uuid).0, Divergence::InSync);

    // Ahead of a device that hasn't caught up yet, but published.
    let b = Device::synced(remote.path());
    b.core.adopt_remote_game(uuid).unwrap();
    a.write("save.dat", "2");
    a.checkpoint(game_a.game.id);
    assert_eq!(relation(&a, uuid).0, Divergence::InSync);
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
    let (parked, original) = remove_version_dir(remote.path(), &v2.uuid);

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

    // The parent arrives late; B's next sync takes it without A doing anything.
    fs::rename(&parked, &original).unwrap();
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
    b.core.update_device("Laptop", None).unwrap();
    let local = b.add_game_at("twice", &a.save_dir());
    b.core
        .update_sync_settings(worldline_core::sync::SyncSettings {
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
    let active = games[0].active_slot.clone().unwrap();
    assert_eq!(active.uuid, local.active_slot.unwrap().uuid);

    // Same-named playthroughs are told apart; the other device's is pointed out.
    let theirs = games[0].slots.iter().find(|s| s.id != active.id).unwrap();
    assert_eq!(active.name, format!("{} (Laptop)", theirs.name));
    assert_eq!(games[0].linked_slot_ids, vec![theirs.id]);
    let summary = games[0]
        .slot_summaries
        .iter()
        .find(|s| s.slot_id == theirs.id)
        .unwrap();
    assert_eq!(
        summary.last_origin_device,
        Some(a.core.device_metadata().device_uuid.to_string())
    );
    let id = games[0].game.id;
    let switched = b.core.switch_slot(id, theirs.id).unwrap();
    assert!(
        switched.linked_slot_ids.is_empty(),
        "found, so no longer pointed out"
    );
    b.core.switch_slot(id, active.id).unwrap();
    assert!(b.game(id).linked_slot_ids.is_empty());

    // A unrelated local game with the same title is reported, not merged.
    let c = Device::new();
    c.write("save.dat", "c");
    c.add_game("Twice");
    c.core
        .update_sync_settings(worldline_core::sync::SyncSettings {
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
        worldline_core::integrity::check(&store, true)
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

#[test]
fn a_registered_device_with_a_missing_state_blocks_cleanup() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    let b = Device::synced(remote.path());
    a.write("save.dat", "x");
    let game = a.add_game("Guarded");
    a.core.refresh_sync().unwrap();
    b.core.adopt_remote_game(uuid_of(&game)).unwrap();
    b.core.push_sync().unwrap();
    let device = b.core.device_metadata().device_uuid.to_string();
    fs::remove_file(
        remote
            .path()
            .join("devices")
            .join(device)
            .join("state.json"),
    )
    .unwrap();
    let error = a.core.run_pruning().unwrap_err();
    assert!(error.to_string().contains("unreadable"), "{error:#}");
}

#[test]
fn a_remotely_deleted_active_slot_keeps_uncaptured_progress() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    let b = Device::synced(remote.path());
    a.write("save.dat", "main");
    let game = a.add_game("Slots");
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
    let adopted = b.core.adopt_remote_game(uuid_of(&game)).unwrap();
    assert_eq!(adopted.active_slot.as_ref().unwrap().name, "Second");

    a.core.delete_slot(second.id, Some(default.id)).unwrap();
    a.core.refresh_sync().unwrap();
    // Device B plays on in the slot A just deleted.
    a.write("save.dat", "uncaptured second progress");
    b.core.refresh_sync().unwrap();

    // The progress A never saw stays live, in a new slot of the same name.
    let game = b.game(adopted.game.id);
    let kept = game.active_slot.as_ref().unwrap();
    assert_eq!(kept.name, "Second");
    assert_ne!(kept.uuid, second.uuid);
    assert_eq!(
        a.read("save.dat").as_deref(),
        Some("uncaptured second progress")
    );
    // It survives the cleanup that collects the deleted slot.
    b.core.run_pruning().unwrap();
    let version = game.last_version.unwrap();
    let store = Store::open_existing(b.core.repository_root()).unwrap();
    let record = store.catalog().unwrap().pop().unwrap();
    let meta = record
        .version(Uuid::parse_str(&version.uuid).unwrap())
        .unwrap();
    assert!(!meta.files.is_empty());
    for file in &meta.files {
        worldline_core::chunks::verify_file(&store, file).unwrap();
    }
}

#[test]
fn adoption_keeps_progress_in_a_retired_folder_it_would_restore() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    let b = Device::synced(remote.path());
    let old = a.saves.path().join("Old Saves");
    fs::create_dir_all(&old).unwrap();
    fs::write(old.join("save.dat"), "old").unwrap();
    a.write("save.dat", "live");
    let dirs = |dirs: &[&Path]| Some(dirs.iter().map(|d| d.to_string_lossy().into()).collect());
    let game = a
        .core
        .save_game(
            GameForm {
                id: None,
                title: "Retired".into(),
            },
            dirs(&[&a.save_dir(), &old]),
        )
        .unwrap();
    a.core
        .save_game(
            GameForm {
                id: Some(game.id),
                title: game.title.clone(),
            },
            dirs(&[&a.save_dir()]),
        )
        .unwrap();
    a.core.push_sync().unwrap();

    // B's current folder is empty, but the retired one holds new progress.
    fs::remove_file(a.save_dir().join("save.dat")).unwrap();
    fs::write(old.join("save.dat"), "retired progress").unwrap();
    b.core
        .adopt_remote_game(Uuid::parse_str(&game.uuid).unwrap())
        .unwrap();
    assert_eq!(
        fs::read_to_string(old.join("save.dat")).unwrap(),
        "retired progress"
    );
}

#[test]
fn a_remotely_deleted_active_slot_without_new_progress_falls_back() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    let b = Device::synced(remote.path());
    a.write("save.dat", "main");
    let game = a.add_game("Slots");
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
    let adopted = b.core.adopt_remote_game(uuid_of(&game)).unwrap();

    a.core.delete_slot(second.id, Some(default.id)).unwrap();
    a.core.refresh_sync().unwrap();
    // B's live saves are what A deleted: B loads the remaining slot.
    a.write("save.dat", "second");
    b.core.refresh_sync().unwrap();

    let game = b.game(adopted.game.id);
    assert_eq!(game.active_slot.as_ref().unwrap().name, "Default Slot");
    assert_eq!(game.slots.len(), 1);
    assert_eq!(a.read("save.dat").as_deref(), Some("main"));
}

/// A game with "Default Slot" ("main") and "Second" ("second", active),
/// adopted by `b`. Returns B's game id and A's two slots.
fn second_slot_on_both(a: &Device, b: &Device) -> (i32, Slot, Slot) {
    a.write("save.dat", "main");
    let game = a.add_game("Slots");
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
    let adopted = b.core.adopt_remote_game(uuid_of(&game)).unwrap();
    (adopted.game.id, default, second)
}

#[test]
fn offline_checkpoints_in_a_remotely_deleted_slot_are_kept() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    let b = Device::synced(remote.path());
    let (id, default, second) = second_slot_on_both(&a, &b);
    // B checkpoints offline, on two branches; A deletes the slot without
    // receiving them.
    a.write("save.dat", "offline 1");
    let fork = b.checkpoint(id);
    a.write("save.dat", "offline 2");
    b.checkpoint(id);
    b.core.restore_version(fork.id).unwrap();
    a.write("save.dat", "offline 3");
    b.checkpoint(id);
    a.core.delete_slot(second.id, Some(default.id)).unwrap();
    a.core.refresh_sync().unwrap();
    a.write("save.dat", "offline 3");
    b.core.refresh_sync().unwrap();

    let game = b.game(id);
    let kept = game.active_slot.as_ref().unwrap();
    assert_eq!(kept.name, "Second");
    assert_ne!(kept.uuid, second.uuid);
    assert_eq!(a.read("save.dat").as_deref(), Some("offline 3"));
    b.core.run_pruning().unwrap();
    let versions = b.core.get_slot(kept.id).unwrap().versions;
    assert_eq!(versions.len(), 3, "every offline checkpoint is kept");
    let store = Store::open_existing(b.core.repository_root()).unwrap();
    let record = store.catalog().unwrap().pop().unwrap();
    let metas: Vec<_> = versions
        .iter()
        .map(|version| {
            record
                .version(Uuid::parse_str(&version.uuid).unwrap())
                .unwrap()
                .clone()
        })
        .collect();
    for file in metas.iter().flat_map(|meta| &meta.files) {
        worldline_core::chunks::verify_file(&store, file).unwrap();
    }
    // Both later checkpoints still branch from the first.
    let root = metas
        .iter()
        .find(|meta| meta.parent_uuid.is_none())
        .unwrap();
    assert!(
        metas
            .iter()
            .filter(|meta| meta.uuid != root.uuid)
            .all(|meta| meta.parent_uuid == Some(root.uuid))
    );
    assert_eq!(
        game.last_version.unwrap().uuid,
        versions
            .iter()
            .max_by_key(|v| v.version_number)
            .unwrap()
            .uuid,
        "the live branch stays active"
    );
}

/// The store metadata of every version in `slot`.
fn slot_metas(device: &Device, slot: i32) -> Vec<worldline_core::store::VersionMeta> {
    let store = Store::open_existing(device.core.repository_root()).unwrap();
    let record = store.catalog().unwrap().pop().unwrap();
    device
        .core
        .get_slot(slot)
        .unwrap()
        .versions
        .iter()
        .map(|version| {
            record
                .version(Uuid::parse_str(&version.uuid).unwrap())
                .unwrap()
                .clone()
        })
        .collect()
}

#[test]
fn kept_offline_checkpoints_follow_ancestry_not_clocks() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    let b = Device::synced(remote.path());
    let (id, default, second) = second_slot_on_both(&a, &b);
    a.write("save.dat", "offline 1");
    let parent = b.checkpoint(id);
    a.write("save.dat", "offline 2");
    b.checkpoint(id);
    // A clock that ran fast dated the parent after its child.
    for entry in walk(&b.data.path().join("repository")) {
        if entry.file_name().is_some_and(|n| n == "metadata.json")
            && entry.to_string_lossy().contains(&parent.folder_slug)
        {
            let mut meta: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(&entry).unwrap()).unwrap();
            meta["created_at"] = "2099-01-01T00:00:00Z".into();
            fs::write(&entry, serde_json::to_string(&meta).unwrap()).unwrap();
        }
    }
    b.core.reindex().unwrap();
    a.core.delete_slot(second.id, Some(default.id)).unwrap();
    a.core.refresh_sync().unwrap();
    a.write("save.dat", "offline 2");
    b.core.refresh_sync().unwrap();

    let kept = b.game(id).active_slot.unwrap();
    let metas = slot_metas(&b, kept.id);
    assert_eq!(metas.len(), 2);
    let root = metas.iter().find(|m| m.parent_uuid.is_none()).unwrap();
    assert!(
        metas.iter().any(|m| m.parent_uuid == Some(root.uuid)),
        "the child still hangs off its parent"
    );
    assert!(
        root.created_at.to_rfc3339().starts_with("2099"),
        "copies keep their capture time"
    );
}

#[test]
fn a_restored_seen_version_is_not_hung_under_an_offline_tip() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    let b = Device::synced(remote.path());
    let (id, default, second) = second_slot_on_both(&a, &b);
    let seen = b.game(id).last_version.unwrap();
    a.write("save.dat", "offline");
    b.checkpoint(id);
    b.core.restore_version(seen.id).unwrap();
    a.core.delete_slot(second.id, Some(default.id)).unwrap();
    a.core.refresh_sync().unwrap();
    a.write("save.dat", "second");
    b.core.refresh_sync().unwrap();

    let game = b.game(id);
    let kept = game.active_slot.as_ref().unwrap();
    assert_eq!(a.read("save.dat").as_deref(), Some("second"));
    let metas = slot_metas(&b, kept.id);
    assert_eq!(metas.len(), 2, "the offline tip and the restored saves");
    let live = game.last_version.unwrap();
    let live = metas.iter().find(|m| m.uuid.to_string() == live.uuid);
    assert_eq!(live.unwrap().parent_uuid, None);
}

#[test]
fn emptied_saves_beside_offline_checkpoints_stay_monitored() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    let b = Device::synced(remote.path());
    let (id, default, second) = second_slot_on_both(&a, &b);
    let seen = b.game(id).last_version.unwrap();
    a.write("save.dat", "offline");
    b.checkpoint(id);
    b.core.restore_version(seen.id).unwrap();
    a.core.delete_slot(second.id, Some(default.id)).unwrap();
    a.core.refresh_sync().unwrap();
    fs::remove_file(a.save_dir().join("save.dat")).unwrap();
    b.core.refresh_sync().unwrap();

    let game = b.game(id);
    assert_eq!(a.read("save.dat"), None);
    assert_eq!(game.versions.len(), 2);
    assert!(game.last_version.is_some(), "the empty saves are recorded");
}

#[test]
fn retired_offline_checkpoints_are_not_resurrected() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    let b = Device::synced(remote.path());
    let (id, default, second) = second_slot_on_both(&a, &b);
    for n in 1..=4 {
        a.write("save.dat", &format!("offline {n}"));
        b.checkpoint(id);
    }
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
    a.core.delete_slot(second.id, Some(default.id)).unwrap();
    a.core.refresh_sync().unwrap();
    a.write("save.dat", "offline 4");
    b.core.refresh_sync().unwrap();

    let kept = b.game(id).active_slot.unwrap();
    assert_eq!(kept.name, "Second");
    let versions = b.core.get_slot(kept.id).unwrap().versions;
    assert!(versions.iter().all(|v| !v.retired));
    assert!(versions.len() < 4, "retired checkpoints stay retired");
}

#[test]
fn emptied_saves_in_a_remotely_deleted_slot_are_kept() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    let b = Device::synced(remote.path());
    let (id, default, second) = second_slot_on_both(&a, &b);
    a.core.delete_slot(second.id, Some(default.id)).unwrap();
    a.core.refresh_sync().unwrap();
    // B deleted its saves after the last version A saw.
    fs::remove_file(a.save_dir().join("save.dat")).unwrap();
    b.core.refresh_sync().unwrap();

    assert_eq!(a.read("save.dat"), None);
    assert_eq!(b.game(id).active_slot.unwrap().name, "Second");
}

#[test]
fn concurrent_deletions_of_every_slot_leave_a_live_slot() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    let b = Device::synced(remote.path());
    let (id, _, second) = second_slot_on_both(&a, &b);
    // B deletes the slot A falls back to; A deletes the one B plays in.
    let b_default = b.game(id).slots.into_iter().find(|s| s.name != "Second");
    b.core.delete_slot(b_default.unwrap().id, None).unwrap();
    let a_game = a.game_by_title("Slots").unwrap();
    let a_default = a_game.slots.iter().find(|s| s.name != "Second").unwrap();
    a.core.delete_slot(second.id, Some(a_default.id)).unwrap();
    a.core.refresh_sync().unwrap();
    a.write("save.dat", "b progress");
    b.core.refresh_sync().unwrap();

    let game = b.game(id);
    let live = game.active_slot.as_ref().expect("a live slot");
    assert_eq!(live.name, "Second");
    assert_eq!(a.read("save.dat").as_deref(), Some("b progress"));
    assert!(game.last_version.is_some(), "the progress is captured");
}

fn maintenance_pointers(remote: &Path) -> std::collections::HashSet<Uuid> {
    let mut pointed = std::collections::HashSet::new();
    for device in fs::read_dir(remote.join("devices")).unwrap() {
        let state: worldline_core::sync::DeviceState =
            worldline_core::store::read_json(&device.unwrap().path().join("state.json")).unwrap();
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
        worldline_core::integrity::check(&store, true)
            .unwrap()
            .is_ok()
    );
    b.core.refresh_sync().unwrap();
    assert_eq!(b.core.get_games().unwrap()[0].versions.len(), 1);
}

// ---- the background sync loop ----------------------------------------------

/// Opens `data` with the watcher and the sync loop, syncing with `remote`.
fn open_running(data: &Path, remote: Option<&Path>) -> (Arc<WorldlineCore>, NotificationReceiver) {
    worldline_core::sync::save_settings(
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
    WorldlineCore::open_shared(data, options).unwrap()
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
fn running_with_game(remote: &Path) -> (Device, Arc<WorldlineCore>, NotificationReceiver, i32) {
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
fn the_loop_announces_every_sync_and_push_before_its_report() {
    let remote = tempfile::tempdir().unwrap();
    let (device, core, rx, game) = running_with_game(remote.path());
    device.write("save.dat", "2");
    core.checkpoint(game).unwrap();
    wait_until("the checkpoint on the remote", || {
        remote_version_count(remote.path()) == 2
    });
    core.shutdown();
    let events: Vec<&str> = rx
        .iter()
        .filter_map(|n| match n {
            Notification::SyncStarted => Some("started"),
            Notification::SyncCompleted(_) => Some("completed"),
            Notification::SyncFailed(_) => Some("failed"),
            _ => None,
        })
        .collect();
    // At least the start-up sync and the final push (the checkpoint's push
    // can coalesce with the start-up sync): each announced first.
    assert!(events.len() >= 4, "{events:?}");
    for pair in events.chunks(2) {
        assert_eq!(pair, ["started", "completed"], "{events:?}");
    }
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
    let unreachable = Some(ErrorKind::SyncFolderUnreachable);
    wait_for(
        &rx,
        "the failure report",
        |n| matches!(n, Notification::SyncFailed(failure) if failure.kind == unreachable),
    );
    // Pruning bails on the same folder; front ends tell it apart by its kind.
    wait_for(
        &rx,
        "the pruning failure",
        |n| matches!(n, Notification::PruningFailed(failure) if failure.kind == unreachable),
    );
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

    let settings = a.core.get_pruning_settings().unwrap();
    let preview = a.core.preview_pruning(&settings).unwrap();
    assert_eq!(preview.live_bytes_before, remote_size);
}

#[test]
fn previewing_unsaved_settings_uses_them_and_writes_nothing() {
    let remote = tempfile::tempdir().unwrap();
    let device = Device::synced(remote.path());
    device.write("save.dat", "1");
    let game = device.add_game("Previewed");
    for n in 2..=4 {
        device.write("save.dat", &n.to_string());
        device.checkpoint(game.game.id);
    }
    device.core.push_sync().unwrap();
    // Age the versions a month apart on both sides: the saved tiers keep them all.
    for entry in walk(&device.data.path().join("repository"))
        .into_iter()
        .chain(walk(remote.path()))
    {
        if entry.file_name().is_some_and(|n| n == "metadata.json")
            && entry.to_string_lossy().contains("versions")
        {
            let mut meta: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(&entry).unwrap()).unwrap();
            meta["created_at"] = format!("2020-0{}-01T00:00:00Z", meta["version_number"]).into();
            fs::write(&entry, serde_json::to_string(&meta).unwrap()).unwrap();
        }
    }
    device.core.reindex().unwrap();
    let saved = device.core.get_pruning_settings().unwrap();
    let remote_settings = remote.path().join("settings").join("retention.json");
    let before = fs::read(&remote_settings).ok();
    assert!(
        device
            .core
            .preview_pruning(&saved)
            .unwrap()
            .retire_versions
            .is_empty()
    );

    let strict = RetentionSettings {
        max_bytes: Some(1),
        ..saved
    };
    assert_eq!(
        device
            .core
            .preview_pruning(&strict)
            .unwrap()
            .retire_versions
            .len(),
        2,
        "everything but the root and the active version"
    );
    assert_eq!(device.core.get_pruning_settings().unwrap(), saved);
    assert!(!device.data.path().join("retention_settings.json").exists());
    assert_eq!(fs::read(&remote_settings).ok(), before);
    assert!(device.core.retired_versions().unwrap().is_empty());

    let overlapping = RetentionSettings {
        keep_all_hours: saved.hourly_until_hours + 1,
        ..saved
    };
    assert!(device.core.preview_pruning(&overlapping).is_err());
}

#[test]
fn an_unreachable_sync_folder_has_its_own_error_kind() {
    let remote = tempfile::tempdir().unwrap();
    let device = Device::synced(remote.path());
    let missing = remote.path().join("unplugged");
    let error = device
        .core
        .update_sync_settings(SyncSettings {
            remote_path: Some(missing.to_string_lossy().into_owned()),
        })
        .unwrap_err();
    assert_eq!(error_kind(&error), Some(ErrorKind::SyncFolderUnreachable));

    let later = remote.path().join("later");
    fs::create_dir_all(&later).unwrap();
    let synced = Device::synced(&later);
    fs::remove_dir_all(&later).unwrap();
    for error in [
        synced.core.refresh_sync().unwrap_err(),
        synced.core.push_sync().unwrap_err(),
    ] {
        assert_eq!(error_kind(&error), Some(ErrorKind::SyncFolderUnreachable));
    }
    assert_eq!(error_kind(&device.core.get_game(999).unwrap_err()), None);
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

#[test]
fn one_games_status_is_read_as_it_is_now_without_pulling() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    a.write("save.dat", "1");
    let game_a = a.add_game("Now");
    let uuid = uuid_of(&game_a);
    a.core.refresh_sync().unwrap();
    let b = Device::synced(remote.path());
    let game_b = b.core.adopt_remote_game(uuid).unwrap();
    let v1 = game_b.versions[0].id;
    let local = |b: &Device| Uuid::parse_str(&b.active_version(game_b.game.id).uuid).unwrap();

    // A plays on and publishes. B hasn't pulled: its index lacks the tip, the remote line has it.
    a.write("save.dat", "2");
    let v2 = Uuid::parse_str(&a.checkpoint(game_a.game.id).uuid).unwrap();
    a.core.push_sync().unwrap();
    a.write("save.dat", "1"); // B's machine still holds version 1
    let (status, parents) = b.core.game_sync_status(uuid).unwrap().unwrap();
    assert_eq!(status.relation, Divergence::FastForward);
    assert_eq!(status.remote_version_uuid, Some(v2));
    assert_eq!(status.remote_version_id, None, "not pulled yet");
    assert!(status.remote_device.is_some());
    assert!(worldline_core::sync::is_ancestor(local(&b), v2, &parents));

    // Applied: in sync at once, with no new report.
    let (_, target) = relation(&b, uuid);
    b.core.apply_remote_version(target.unwrap()).unwrap();
    let (status, _) = b.core.game_sync_status(uuid).unwrap().unwrap();
    assert_eq!(status.relation, Divergence::InSync);

    // Restoring the older version after that puts B behind again, at once.
    b.core.restore_version(v1).unwrap();
    let (status, _) = b.core.game_sync_status(uuid).unwrap().unwrap();
    assert_eq!(status.relation, Divergence::FastForward);
    assert_eq!(status.remote_version_uuid, Some(v2));

    // A capture of its own there makes it a new line: no longer just behind.
    a.write("save.dat", "B3");
    b.checkpoint(game_b.game.id);
    let (status, _) = b.core.game_sync_status(uuid).unwrap().unwrap();
    assert_ne!(status.relation, Divergence::FastForward);

    // Another remote without the game has no status for it; no remote is an error.
    let other = tempfile::tempdir().unwrap();
    b.core
        .update_sync_settings(SyncSettings {
            remote_path: Some(other.path().to_string_lossy().into_owned()),
        })
        .unwrap();
    assert!(b.core.game_sync_status(uuid).unwrap().is_none());
    b.core
        .update_sync_settings(SyncSettings { remote_path: None })
        .unwrap();
    assert!(b.core.game_sync_status(uuid).is_err());
}

#[test]
fn a_games_push_publishes_only_that_game() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    a.write("save.dat", "1");
    let pushed = a.add_game("Pushed");
    let other_dir = a.saves.path().join("Other Saves");
    fs::create_dir_all(&other_dir).unwrap();
    fs::write(other_dir.join("save.dat"), "1").unwrap();
    let other = a.add_game_at("Other", &other_dir);
    a.core.refresh_sync().unwrap();

    a.write("save.dat", "2");
    a.checkpoint(pushed.game.id);
    fs::write(other_dir.join("save.dat"), "2").unwrap();
    a.checkpoint(other.game.id);
    a.core
        .push_games(&std::collections::HashSet::from([uuid_of(&pushed)]))
        .unwrap();
    let store = Store::open_existing(remote.path()).unwrap();
    let versions = |game| store.game(game).unwrap().unwrap().versions().count();
    assert_eq!(versions(uuid_of(&pushed)), 2);
    assert_eq!(versions(uuid_of(&other)), 1, "left for the next full push");

    a.core.push_sync().unwrap();
    assert_eq!(versions(uuid_of(&other)), 2);
}

#[test]
fn a_games_push_keeps_the_last_status_of_the_others() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    a.write("save.dat", "1");
    let shared = a.add_game("Shared");
    let uuid = uuid_of(&shared);
    a.core.refresh_sync().unwrap();
    let b = Device::synced(remote.path());
    b.core.adopt_remote_game(uuid).unwrap();
    let own_dir = b.saves.path().join("Own Saves");
    fs::create_dir_all(&own_dir).unwrap();
    fs::write(own_dir.join("save.dat"), "1").unwrap();
    let own = b.add_game_at("Own", &own_dir);

    a.write("save.dat", "2");
    a.checkpoint(shared.game.id);
    a.core.push_sync().unwrap();
    a.write("save.dat", "1"); // B's machine still holds version 1
    assert_eq!(relation(&b, uuid).0, Divergence::FastForward);

    // A moves on again; B pushes only its own game before any full sync.
    a.write("save.dat", "3");
    a.checkpoint(shared.game.id);
    a.core.push_sync().unwrap();
    a.write("save.dat", "1");
    fs::write(own_dir.join("save.dat"), "2").unwrap();
    b.checkpoint(own.game.id);
    let pushed = std::collections::HashSet::from([uuid_of(&own)]);
    b.core.push_games(&pushed).unwrap();
    let report = b.core.pushed_status(&pushed).unwrap();
    let status = report.games.iter().find(|s| s.game_uuid == uuid).unwrap();
    assert_eq!(
        status.relation,
        Divergence::FastForward,
        "not shown as in sync"
    );
}
