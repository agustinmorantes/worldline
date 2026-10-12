//! Full syncs that read and push only what changed (ADR 0012). Each sync's
//! work is counted from the phase lines it logs: how many games it read from
//! the remote, how many it pulled, and how many it left unchanged when
//! pushing.

mod common;

use common::Device;
use std::cell::RefCell;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;
use worldline_core::models::{FullGame, GameForm, MonitoringSettings};
use worldline_core::store::{RetirementMarker, Store};
use worldline_core::sync::{SyncReport, SyncSettings};
use worldline_core::{OpenOptions, WorldlineCore};

fn stamp_fixture() -> worldline_core::store::GameRecord {
    let device = Device::new();
    let game = add_game_in(&device, "Stamp");
    fs::write(device.saves.path().join("Stamp/save.dat"), "second").unwrap();
    device.checkpoint(game.game.id);
    Store::open_existing(device.core.repository_root())
        .unwrap()
        .game(uuid_of(&game))
        .unwrap()
        .unwrap()
}

#[test]
fn activity_on_another_version_does_not_satisfy_a_stamp() {
    let mut writer = stamp_fixture();
    let mut reader = writer.clone();
    let at = chrono::Utc::now();
    writer.slots[0].versions[0].last_played_at = None;
    writer.slots[0].versions[1].last_played_at = Some(at);
    reader.slots[0].versions[0].last_played_at = Some(at);
    reader.slots[0].versions[1].last_played_at = None;
    let stamp = worldline_core::sync::GameStamp::of(
        &writer,
        &Default::default(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    assert!(
        !stamp.arrived_in(&reader),
        "equal counts of different versions are not arrival"
    );
    reader.slots[0].versions[1].last_played_at = Some(at);
    assert!(stamp.arrived_in(&reader));
}

#[test]
fn a_screenshot_of_another_version_does_not_satisfy_a_stamp() {
    let game = stamp_fixture();
    let first = game.slots[0].versions[0].uuid;
    let second = game.slots[0].versions[1].uuid;
    let stamp = worldline_core::sync::GameStamp::of(
        &game,
        &Default::default(),
        &std::collections::BTreeSet::from([second]),
        &Default::default(),
    )
    .unwrap();
    let mut shots = std::collections::BTreeSet::from([first]);
    assert!(!stamp.screenshots_arrived(&game, &shots, &Default::default()));
    shots.insert(second);
    assert!(stamp.screenshots_arrived(&game, &shots, &Default::default()));
}

#[test]
fn a_parent_cycle_does_not_satisfy_a_stamp() {
    let mut game = stamp_fixture();
    let stamp = worldline_core::sync::GameStamp::of(
        &game,
        &Default::default(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    game.slots[0].versions[0].parent_uuid = Some(game.slots[0].versions[1].uuid);
    assert!(!stamp.arrived_in(&game));
}

#[test]
fn a_newer_timestamp_does_not_hide_a_missing_sticky_tombstone() {
    let mut writer = stamp_fixture();
    let mut reader = writer.clone();
    let at = chrono::Utc::now();
    writer.meta.deleted_at = Some(at);
    writer.slots[0].meta.deleted_at = Some(at);
    writer.slots[0].meta.seen_versions = writer.slots[0].versions.iter().map(|v| v.uuid).collect();
    reader.meta.updated_at = at + chrono::Duration::days(1);
    reader.slots[0].meta.updated_at = reader.meta.updated_at;
    let stamp = worldline_core::sync::GameStamp::of(
        &writer,
        &Default::default(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    assert!(!stamp.arrived_in(&reader));
    reader.meta.deleted_at = Some(at);
    assert!(!stamp.arrived_in(&reader));
    reader.slots[0].meta.deleted_at = Some(at);
    assert!(
        !stamp.arrived_in(&reader),
        "the deletion's observed versions must arrive too"
    );
    reader.slots[0].meta.seen_versions = writer.slots[0].meta.seen_versions.clone();
    assert!(stamp.arrived_in(&reader));
}

thread_local! { static LINES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) }; }

/// Keeps this thread's sync phase lines (each test syncs on its own thread).
struct Capture;

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }
    fn log(&self, record: &log::Record<'_>) {
        let line = record.args().to_string();
        if line.starts_with("Full sync took ") || line.starts_with("Sync push took ") {
            LINES.with(|lines| lines.borrow_mut().push(line));
        }
    }
    fn flush(&self) {}
}

fn capture() {
    static LOGGER: Capture = Capture;
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        log::set_logger(&LOGGER).unwrap();
        log::set_max_level(log::LevelFilter::Info);
    });
}

/// What one full sync did.
struct Synced {
    /// Read every game (a check, a new or invalid cache, an older client).
    full: bool,
    read: usize,
    pulled: usize,
    unchanged: usize,
    report: SyncReport,
}

/// The number in `label (…N…)` of a phase line, skipping a `full…, ` prefix.
fn count(line: &str, label: &str) -> usize {
    let start = line.find(&format!("{label} (")).expect(label) + label.len() + 2;
    let rest = &line[start..];
    let rest = rest.find(", ").map_or(rest, |at| {
        if rest[..at].starts_with("full") {
            &rest[at + 2..]
        } else {
            rest
        }
    });
    rest.chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap()
}

fn sync(device: &Device) -> Synced {
    capture();
    LINES.with(|lines| lines.borrow_mut().clear());
    let report = device.core.refresh_sync().unwrap();
    let lines = LINES.with(|lines| lines.take());
    let full = lines
        .iter()
        .find(|line| line.starts_with("Full sync took "))
        .expect("a full sync line");
    let push = lines
        .iter()
        .find(|line| line.starts_with("Sync push took "))
        .expect("a push line");
    Synced {
        full: full.contains("catalog (full, "),
        read: count(full, "catalog"),
        pulled: count(full, "pull"),
        unchanged: count(push, "games"),
        report,
    }
}

fn uuid_of(game: &FullGame) -> Uuid {
    Uuid::parse_str(&game.game.uuid).unwrap()
}

/// A game tracking its own folder under the device's saves.
fn add_game_in(device: &Device, title: &str) -> FullGame {
    let dir = device.saves.path().join(title);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("save.dat"), title).unwrap();
    device.add_game_at(title, &dir)
}

/// A device synced twice with nothing changing in between: the second sync
/// is incremental, so later ones start from a settled cache.
fn settled(device: &Device) {
    sync(device);
    let second = sync(device);
    assert!(!second.full, "the second sync trusts the cache");
}

/// Moves a version's folder out of the remote; returns where it went and
/// where it was.
fn park_version(remote: &Path, version_uuid: &str) -> (PathBuf, PathBuf) {
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

fn chunk_files(remote: &Path) -> HashSet<PathBuf> {
    let mut files = HashSet::new();
    for shard in fs::read_dir(remote.join("chunks")).unwrap() {
        for chunk in fs::read_dir(shard.unwrap().path()).unwrap() {
            files.insert(chunk.unwrap().path());
        }
    }
    files
}

/// The device restarted: the cache in memory is gone, the one on disk kept.
fn reopen(device: Device) -> Device {
    let Device { core, data, saves } = device;
    drop(core);
    let (core, _) = WorldlineCore::open(data.path(), OpenOptions::default()).unwrap();
    Device { core, data, saves }
}

fn cache_index(device: &Device) -> PathBuf {
    device
        .data
        .path()
        .join("cache")
        .join("sync")
        .join("remote.json")
}

/// Edits a JSON file in place.
fn edit_json(path: &Path, change: impl FnOnce(&mut serde_json::Value)) {
    let mut value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    change(&mut value);
    fs::write(path, serde_json::to_string_pretty(&value).unwrap()).unwrap();
}

fn state_path(remote: &Path, device: &Device) -> PathBuf {
    remote
        .join("devices")
        .join(device.core.device_metadata().device_uuid.to_string())
        .join("state.json")
}

#[test]
fn idle_syncs_read_and_push_only_what_changed() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    let one = add_game_in(&a, "One");
    add_game_in(&a, "Two");
    add_game_in(&a, "Three");
    let first = sync(&a);
    assert!(first.full, "nothing cached yet");
    assert_eq!(first.unchanged, 0, "every game is pushed");

    let b = Device::synced(remote.path());
    b.core.adopt_remote_game(uuid_of(&one)).unwrap();
    settled(&b);
    let after_b = sync(&a);
    assert!(!after_b.full);
    assert_eq!(after_b.read, 1, "only the game B published");

    let idle = sync(&a);
    assert!(!idle.full);
    assert_eq!(
        (idle.read, idle.pulled, idle.unchanged),
        (0, 0, 3),
        "an idle sync reads, pulls and pushes nothing"
    );
    assert!(
        idle.report.problems.is_empty(),
        "{:?}",
        idle.report.problems
    );
    assert_eq!(
        idle.report.remote_games.len(),
        3,
        "the report still lists every game"
    );
    let idle_b = sync(&b);
    assert_eq!((idle_b.read, idle_b.pulled, idle_b.unchanged), (0, 0, 1));

    // A plays One: only One is pushed, and only One is read on B.
    fs::write(a.saves.path().join("One").join("save.dat"), "level 2").unwrap();
    let v2 = a.checkpoint(one.game.id);
    let changed = sync(&a);
    assert_eq!((changed.read, changed.unchanged), (0, 2));
    let again = sync(&a);
    assert_eq!(
        (again.read, again.unchanged),
        (0, 3),
        "its own push is not read back"
    );
    let on_b = sync(&b);
    assert_eq!((on_b.read, on_b.pulled), (1, 1));
    let game_b = b.game_by_title("One").unwrap();
    assert!(
        b.core
            .get_slot(game_b.active_slot.unwrap().id)
            .unwrap()
            .versions
            .iter()
            .any(|v| v.uuid == v2.uuid)
    );
}

#[test]
fn scoped_pushes_publish_stamps() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    let one = add_game_in(&a, "One");
    add_game_in(&a, "Two");
    settled(&a);
    let b = Device::synced(remote.path());
    b.core.adopt_remote_game(uuid_of(&one)).unwrap();
    settled(&b);
    settled(&a);

    fs::write(a.saves.path().join("One").join("save.dat"), "level 2").unwrap();
    let v2 = a.checkpoint(one.game.id);
    a.core.push_games(&HashSet::from([uuid_of(&one)])).unwrap();
    let status = a
        .core
        .pushed_status(&HashSet::from([uuid_of(&one)]))
        .unwrap();
    assert_eq!(status.games.len(), 2);
    let next = sync(&a);
    assert_eq!(
        (next.read, next.unchanged),
        (0, 2),
        "the scoped push counts"
    );
    let on_b = sync(&b);
    assert_eq!((on_b.read, on_b.pulled), (1, 1));
    assert!(b.core.get_games().unwrap().iter().any(|game| {
        b.core
            .get_slot(game.active_slot.as_ref().unwrap().id)
            .unwrap()
            .versions
            .iter()
            .any(|v| v.uuid == v2.uuid)
    }));
}

#[test]
fn a_stamp_ahead_of_its_metadata_and_chunks_is_retried_until_they_arrive() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    let game = add_game_in(&a, "Late");
    settled(&a);
    let b = Device::synced(remote.path());
    let adopted = b.core.adopt_remote_game(uuid_of(&game)).unwrap();
    settled(&b);
    let versions_b = || {
        b.core
            .get_slot(b.game(adopted.game.id).active_slot.unwrap().id)
            .unwrap()
            .versions
            .into_iter()
            .map(|v| v.uuid)
            .collect::<HashSet<_>>()
    };

    // A's new state reaches B before the version's metadata does.
    let saves = a.saves.path().join("Late");
    fs::write(saves.join("save.dat"), "level 2, all new bytes").unwrap();
    let chunks_before = chunk_files(remote.path());
    let v2 = a.checkpoint(game.game.id);
    sync(&a);
    let new_chunks: Vec<PathBuf> = chunk_files(remote.path())
        .difference(&chunks_before)
        .cloned()
        .collect();
    assert!(!new_chunks.is_empty());
    let (parked, original) = park_version(remote.path(), &v2.uuid);
    let early = sync(&b);
    assert_eq!(early.read, 1);
    assert!(!versions_b().contains(&v2.uuid));
    let retry = sync(&b);
    assert_eq!(retry.read, 1, "read again although A changed nothing");

    // The metadata arrives, its chunks not yet.
    let parked_chunks: Vec<(PathBuf, PathBuf)> = new_chunks
        .iter()
        .map(|path| {
            let parked = remote.path().join(format!(
                "parked-{}",
                path.file_name().unwrap().to_string_lossy()
            ));
            fs::rename(path, &parked).unwrap();
            (parked, path.clone())
        })
        .collect();
    fs::rename(&parked, &original).unwrap();
    let metadata = sync(&b);
    assert_eq!((metadata.read, metadata.pulled), (1, 1));
    assert!(!versions_b().contains(&v2.uuid), "its chunks are missing");
    assert!(
        !metadata.report.problems.is_empty(),
        "reported as not arrived"
    );
    let waiting = sync(&b);
    assert_eq!(
        (waiting.read, waiting.pulled),
        (0, 1),
        "settled metadata, pulled again"
    );

    for (parked, path) in parked_chunks {
        fs::rename(parked, path).unwrap();
    }
    let arrived = sync(&b);
    assert_eq!(arrived.pulled, 1);
    assert!(
        arrived.report.problems.is_empty(),
        "{:?}",
        arrived.report.problems
    );
    assert!(versions_b().contains(&v2.uuid));
    let done = sync(&b);
    assert_eq!((done.read, done.pulled), (0, 0));
}

#[test]
fn missing_chunks_of_an_unchanged_game_are_healed_without_reading_it() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    add_game_in(&a, "Healed");
    settled(&a);
    let chunks = chunk_files(remote.path());
    let lost = chunks.iter().next().unwrap();
    fs::remove_file(lost).unwrap();
    let healed = sync(&a);
    assert_eq!((healed.read, healed.unchanged), (0, 1));
    assert!(lost.is_file(), "re-uploaded from the cached manifest");
}

#[test]
fn edits_deletions_untracking_and_retirements_reach_other_devices() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    let game = add_game_in(&a, "Edited");
    fs::write(a.saves.path().join("Edited").join("save.dat"), "2").unwrap();
    a.checkpoint(game.game.id);
    let other = add_game_in(&a, "Other");
    settled(&a);
    let b = Device::synced(remote.path());
    let on_b = b.core.adopt_remote_game(uuid_of(&game)).unwrap().game.id;
    let other_b = b.core.adopt_remote_game(uuid_of(&other)).unwrap().game.id;
    settled(&b);
    settled(&a);

    // A rename, then a tag.
    a.core
        .save_game(
            GameForm {
                id: Some(game.game.id),
                title: "Renamed".into(),
            },
            None,
        )
        .unwrap();
    let first = a.active_version(game.game.id);
    a.core.tag_version(first.id, "boss".into()).unwrap();
    assert_eq!(sync(&a).unchanged, 1);
    let edited = sync(&b);
    assert_eq!((edited.read, edited.pulled), (1, 1));
    assert_eq!(b.game(on_b).game.title, "Renamed");
    let tagged = b
        .core
        .get_slot(b.game(on_b).active_slot.unwrap().id)
        .unwrap()
        .versions
        .into_iter()
        .find(|v| v.uuid == first.uuid)
        .unwrap();
    assert_eq!(tagged.tag.as_deref(), Some("boss"));

    // A retirement written by a device that changed nothing else.
    let older = b
        .core
        .get_slot(b.game(on_b).active_slot.unwrap().id)
        .unwrap()
        .versions
        .into_iter()
        .find(|v| v.uuid != first.uuid)
        .unwrap();
    let store = Store::open_existing(remote.path()).unwrap();
    let slot_uuid = Uuid::parse_str(&b.game(on_b).active_slot.unwrap().uuid).unwrap();
    store
        .write_marker(&RetirementMarker {
            version_uuid: Uuid::parse_str(&older.uuid).unwrap(),
            slot_uuid,
            game_uuid: uuid_of(&game),
            retired_at: chrono::Utc::now(),
        })
        .unwrap();
    let retired = sync(&b);
    assert_eq!(
        (retired.read, retired.pulled),
        (0, 1),
        "pulled for its marker"
    );
    assert!(b.core.get_version(older.id).unwrap().version.retired);

    // B untracks Other: A's changes to it are no longer pulled here.
    b.core.untrack_game(other_b).unwrap();
    fs::write(a.saves.path().join("Other").join("save.dat"), "2").unwrap();
    a.checkpoint(other.game.id);
    sync(&a);
    let untracked = sync(&b);
    // Both are read: A's stamp of the edited game now covers B's marker.
    assert_eq!(
        (untracked.read, untracked.pulled),
        (2, 1),
        "Other is not pulled"
    );

    // A deletes the game.
    a.core.delete_game(game.game.id).unwrap();
    assert_eq!(sync(&a).unchanged, 1);
    let deleted = sync(&b);
    assert_eq!(deleted.read, 1);
    assert!(
        !deleted
            .report
            .remote_games
            .iter()
            .any(|g| g.uuid == uuid_of(&game)),
        "the tombstone arrived"
    );
}

#[test]
fn a_lost_or_foreign_cache_falls_back_to_a_full_read() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    add_game_in(&a, "Cached");
    settled(&a);

    let a = reopen(a);
    fs::remove_dir_all(a.data.path().join("cache").join("sync")).unwrap();
    let a = reopen(a);
    assert!(sync(&a).full, "missing cache");
    assert!(!sync(&a).full);

    fs::write(cache_index(&a), "{ not json").unwrap();
    let a = reopen(a);
    assert!(sync(&a).full, "corrupt cache");
    assert!(!sync(&a).full);

    // The same folder now holding another store.
    edit_json(&remote.path().join("metadata.json"), |meta| {
        meta["created_at"] = "2020-01-01T00:00:00Z".into();
    });
    assert!(sync(&a).full, "recreated remote");
    assert!(!sync(&a).full);

    // Another folder, then back.
    let other = tempfile::tempdir().unwrap();
    let switch = |path: &Path| {
        a.core
            .update_sync_settings(SyncSettings {
                remote_path: Some(path.to_string_lossy().into_owned()),
            })
            .unwrap();
    };
    switch(other.path());
    assert!(sync(&a).full, "another remote");
    switch(remote.path());
    let back = sync(&a);
    assert!(back.full, "the cache was for the other remote");
    assert!(
        back.report.problems.is_empty(),
        "{:?}",
        back.report.problems
    );
}

#[test]
fn a_restart_keeps_trusting_the_cache() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    add_game_in(&a, "Restarted");
    settled(&a);
    let a = reopen(a);
    let restarted = sync(&a);
    assert!(!restarted.full);
    assert_eq!((restarted.read, restarted.unchanged), (0, 1));
}

#[test]
fn older_clients_and_unreadable_states_make_every_sync_read_everything() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    let game = add_game_in(&a, "Mixed");
    settled(&a);
    let b = Device::synced(remote.path());
    let on_b = b.core.adopt_remote_game(uuid_of(&game)).unwrap().game.id;
    settled(&b);

    // An older A pushes a version and rewrites its state without stamps.
    fs::write(a.saves.path().join("Mixed").join("save.dat"), "2").unwrap();
    let v2 = a.checkpoint(game.game.id);
    sync(&a);
    edit_json(&state_path(remote.path(), &a), |state| {
        state.as_object_mut().unwrap().remove("stamps");
    });
    let mixed = sync(&b);
    assert!(mixed.full, "an older client signals nothing");
    assert!(
        b.core
            .get_slot(b.game(on_b).active_slot.unwrap().id)
            .unwrap()
            .versions
            .iter()
            .any(|v| v.uuid == v2.uuid)
    );
    assert!(sync(&b).full, "for as long as it publishes no stamps");

    // Upgraded, A publishes stamps again.
    sync(&a);
    assert!(!sync(&b).full);

    fs::write(state_path(remote.path(), &a), "{ unreadable").unwrap();
    assert!(sync(&b).full, "unknown stamps");
}

#[test]
fn the_periodic_check_reads_and_pushes_everything_again() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    let game = add_game_in(&a, "Checked");
    fs::write(a.saves.path().join("Checked").join("save.dat"), "2").unwrap();
    let v2 = a.checkpoint(game.game.id);
    settled(&a);

    // Metadata lost behind every stamp's back is not noticed while the
    // cache is trusted...
    let (parked, original) = park_version(remote.path(), &v2.uuid);
    fs::remove_dir_all(parked).unwrap();
    assert_eq!(sync(&a).unchanged, 1);
    assert!(!original.exists());

    // ...and is republished by the check a day later.
    let a = reopen(a);
    edit_json(&cache_index(&a), |index| {
        index["verified_at"] = (chrono::Utc::now() - chrono::Duration::days(2))
            .to_rfc3339()
            .into();
    });
    let a = reopen(a);
    let checked = sync(&a);
    assert!(checked.full);
    assert_eq!(checked.unchanged, 0);
    assert!(original.is_dir(), "republished");
    assert!(!sync(&a).full, "trusted again until the next check");
}

#[test]
fn a_marker_seen_by_a_scoped_push_is_still_pulled_by_the_next_sync() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    let game = add_game_in(&a, "Marked");
    fs::write(a.saves.path().join("Marked").join("save.dat"), "2").unwrap();
    let newest = a.checkpoint(game.game.id);
    settled(&a);
    let b = Device::synced(remote.path());
    let on_b = b.core.adopt_remote_game(uuid_of(&game)).unwrap().game.id;
    settled(&b);

    // Another device retires B's older version and publishes nothing else.
    let slot = b.game(on_b).active_slot.unwrap();
    let older = b
        .core
        .get_slot(slot.id)
        .unwrap()
        .versions
        .into_iter()
        .find(|v| v.uuid != newest.uuid)
        .unwrap();
    Store::open_existing(remote.path())
        .unwrap()
        .write_marker(&RetirementMarker {
            version_uuid: Uuid::parse_str(&older.uuid).unwrap(),
            slot_uuid: Uuid::parse_str(&slot.uuid).unwrap(),
            game_uuid: uuid_of(&game),
            retired_at: chrono::Utc::now(),
        })
        .unwrap();
    // A scoped push sees the marker first.
    b.core.push_games(&HashSet::from([uuid_of(&game)])).unwrap();
    let next = sync(&b);
    assert_eq!(next.pulled, 1, "pulled for the marker");
    assert!(b.core.get_version(older.id).unwrap().version.retired);
}

fn sync_screenshots(device: &Device, on: bool) {
    let settings = device.core.get_monitoring_settings().unwrap();
    device
        .core
        .update_monitoring_settings(MonitoringSettings {
            sync_screenshots: on,
            ..settings
        })
        .unwrap();
}

/// Stores a screenshot as the watcher does once a shot meets its version;
/// returns where it goes on `remote`.
fn attach(device: &Device, version_id: i32, remote: &Path) -> PathBuf {
    let version = device.core.get_version(version_id).unwrap();
    let (game, slot, folder) = (
        &version.game.folder_slug,
        &version.slot.folder_slug,
        &version.version.folder_slug,
    );
    Store::open_existing(device.core.repository_root())
        .unwrap()
        .write_screenshot(game, slot, folder, b"jpeg")
        .unwrap();
    Store::open_existing(remote)
        .unwrap()
        .screenshot_path(game, slot, folder)
}

/// The id on `device` of the version with `uuid`.
fn version_id(device: &Device, uuid: &str) -> i32 {
    device
        .core
        .get_games()
        .unwrap()
        .iter()
        .flat_map(|game| device.game(game.game.id).versions)
        .find(|v| v.uuid == uuid)
        .unwrap()
        .id
}

#[test]
fn a_screenshot_arriving_after_its_stamp_is_pulled_without_another_push() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    let game = add_game_in(&a, "Shots");
    settled(&a);
    let b = Device::synced(remote.path());
    b.core.adopt_remote_game(uuid_of(&game)).unwrap();
    settled(&b);

    fs::write(a.saves.path().join("Shots").join("save.dat"), "2").unwrap();
    let v2 = a.checkpoint(game.game.id);
    let on_remote = attach(&a, v2.id, remote.path());
    sync(&a);
    assert!(on_remote.is_file());
    let parked = remote.path().join("parked.jpg");
    fs::rename(&on_remote, &parked).unwrap();
    let early = sync(&b);
    assert_eq!(early.pulled, 1);
    let id = version_id(&b, &v2.uuid);
    assert!(b.core.screenshot_path(id).unwrap().is_none());

    fs::rename(&parked, &on_remote).unwrap();
    let late = sync(&b);
    assert_eq!((late.read, late.pulled), (0, 1), "pulled again, not read");
    assert!(b.core.screenshot_path(id).unwrap().is_some());
    let done = sync(&b);
    assert_eq!((done.read, done.pulled), (0, 0));
}

#[test]
fn turning_screenshot_sync_on_pulls_the_screenshots_already_there() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    let game = add_game_in(&a, "Later");
    settled(&a);
    let b = Device::synced(remote.path());
    sync_screenshots(&b, false);
    b.core.adopt_remote_game(uuid_of(&game)).unwrap();
    settled(&b);

    fs::write(a.saves.path().join("Later").join("save.dat"), "2").unwrap();
    let v2 = a.checkpoint(game.game.id);
    attach(&a, v2.id, remote.path());
    sync(&a);
    let off = sync(&b);
    assert_eq!(off.pulled, 1);
    let id = version_id(&b, &v2.uuid);
    assert!(b.core.screenshot_path(id).unwrap().is_none(), "left behind");
    let idle = sync(&b);
    assert_eq!(
        (idle.read, idle.pulled),
        (0, 0),
        "nothing missing while off"
    );

    sync_screenshots(&b, true);
    let on = sync(&b);
    assert_eq!(on.pulled, 1);
    assert!(b.core.screenshot_path(id).unwrap().is_some());
    assert_eq!(sync(&b).pulled, 0);
}

/// The metadata file of the version with `uuid` in the store at `root`.
fn version_meta(root: &Path, uuid: &str) -> PathBuf {
    let short = &Uuid::parse_str(uuid).unwrap().simple().to_string()[..8];
    for game in fs::read_dir(root.join("games")).unwrap() {
        for slot in fs::read_dir(game.unwrap().path().join("slots")).unwrap() {
            for version in fs::read_dir(slot.unwrap().path().join("versions")).unwrap() {
                let path = version.unwrap().path();
                if path.file_name().unwrap().to_string_lossy().ends_with(short) {
                    return path.join("metadata.json");
                }
            }
        }
    }
    panic!("version dir not found");
}

#[test]
fn activity_filled_in_after_its_stamp_is_read_again_until_it_arrives() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    let game = add_game_in(&a, "Played");
    // A version first published without activity, as older clients did.
    let local = version_meta(a.core.repository_root(), &game.versions[0].uuid);
    let mut played = serde_json::Value::Null;
    edit_json(&local, |meta| {
        played = meta
            .as_object_mut()
            .unwrap()
            .remove("last_played_at")
            .unwrap();
    });
    assert!(!played.is_null());
    a.core.reindex().unwrap();
    settled(&a);
    let b = Device::synced(remote.path());
    let on_b = b.core.adopt_remote_game(uuid_of(&game)).unwrap().game.id;
    settled(&b);
    assert_eq!(b.game(on_b).last_played_at, None);

    // A fills it in; its state reaches B before the version's metadata does.
    edit_json(&local, |meta| meta["last_played_at"] = played.clone());
    a.core.reindex().unwrap();
    let on_remote = version_meta(remote.path(), &game.versions[0].uuid);
    let before = fs::read(&on_remote).unwrap();
    sync(&a);
    let after = fs::read(&on_remote).unwrap();
    assert_ne!(before, after, "published");
    fs::write(&on_remote, &before).unwrap();
    let early = sync(&b);
    assert_eq!(early.read, 1);
    assert_eq!(b.game(on_b).last_played_at, None);

    fs::write(&on_remote, &after).unwrap();
    let late = sync(&b);
    assert_eq!(late.read, 1, "read again although A changed nothing");
    assert!(b.game(on_b).last_played_at.is_some());
    assert_eq!(sync(&b).read, 0);
}
