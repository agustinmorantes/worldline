//! The one end-to-end test through real filesystem notifications. Scheduling
//! rules are unit-tested with a fake clock in `detection::scheduler`.

mod common;

use chrono::NaiveDateTime;
use std::fs;
use std::time::{Duration, Instant};
use worldline_core::detection::{BranchFrom, CaptureFailure, Notification};
use worldline_core::models::{GameForm, MonitoringSettings};
use worldline_core::{OpenOptions, WorldlineCore};

fn wait_for(
    rx: &std::sync::mpsc::Receiver<Notification>,
    want: impl Fn(&Notification) -> bool,
) -> bool {
    let deadline = Instant::now() + Duration::from_secs(20);
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(notification) if want(&notification) => return true,
            Ok(_) => {}
            Err(_) => return false,
        }
    }
    false
}

#[test]
fn changes_are_captured_and_restores_do_not_create_versions() {
    let data = tempfile::tempdir().unwrap();
    let saves = tempfile::tempdir().unwrap();
    // Below the settings UI's bounds, so the test runs in seconds.
    worldline_core::store::write_json(
        &data.path().join("monitoring_settings.json"),
        &MonitoringSettings {
            version_cooldown_minutes: 0,
            snapshot_debounce_seconds: 0.2,
            ..MonitoringSettings::default()
        },
    )
    .unwrap();
    let (core, rx) = WorldlineCore::open(
        data.path(),
        OpenOptions {
            watch: true,
            debounce_seconds: None,
        },
    )
    .unwrap();
    fs::write(saves.path().join("save.dat"), "first").unwrap();
    let game = core
        .save_game(
            GameForm {
                id: None,
                title: "Watched".into(),
            },
            Some(vec![saves.path().to_string_lossy().into_owned()]),
        )
        .unwrap();
    let first = core.get_game(game.id).unwrap().last_version.unwrap();

    // Wait until the watcher has registered the folder and reports a change.
    let mut created = false;
    for n in 0..20 {
        fs::write(saves.path().join("save.dat"), format!("played {n}")).unwrap();
        if wait_for(&rx, |n| matches!(n, Notification::VersionCreated { .. })) {
            created = true;
            break;
        }
    }
    assert!(created, "no version was captured from a filesystem change");
    let count = core.get_game(game.id).unwrap().versions.len();

    core.restore_version(first.id).unwrap();
    assert_eq!(
        fs::read_to_string(saves.path().join("save.dat")).unwrap(),
        "first"
    );
    std::thread::sleep(Duration::from_secs(2));
    let after = core.get_game(game.id).unwrap();
    // The restore's own pre-restore checkpoint is the only allowed addition.
    assert!(
        after.versions.len() <= count + 1,
        "restore writes were captured as versions"
    );
    assert_eq!(after.last_version.unwrap().id, first.id);
}

#[test]
fn imported_history_is_not_snapshotted_until_a_version_is_chosen() {
    let source = common::Device::new();
    source.write("save.dat", "exported");
    let exported = source.add_game("Imported");
    let out = tempfile::tempdir().unwrap();
    let bundle = out.path().join("bundle.zip");
    source
        .core
        .export_game_history(exported.game.id, &bundle)
        .unwrap();

    let data = tempfile::tempdir().unwrap();
    let saves = tempfile::tempdir().unwrap();
    let control = tempfile::tempdir().unwrap();
    worldline_core::store::write_json(
        &data.path().join("monitoring_settings.json"),
        &MonitoringSettings {
            version_cooldown_minutes: 0,
            snapshot_debounce_seconds: 0.2,
            ..MonitoringSettings::default()
        },
    )
    .unwrap();
    let open = || {
        WorldlineCore::open(
            data.path(),
            OpenOptions {
                watch: true,
                debounce_seconds: None,
            },
        )
        .unwrap()
    };
    let (core, _) = open();
    let summary = WorldlineCore::inspect_history_bundle(&bundle).unwrap();
    let map = summary
        .active_tracked_dirs
        .iter()
        .map(|dir| (dir.uuid, saves.path().to_path_buf()))
        .collect();
    core.import_game_history(&bundle, &map).unwrap();
    fs::write(control.path().join("save.dat"), "one").unwrap();
    let control_game = core
        .save_game(
            GameForm {
                id: None,
                title: "Control".into(),
            },
            Some(vec![control.path().to_string_lossy().into_owned()]),
        )
        .unwrap();
    drop(core);

    // Both folders change while the app is closed; only the chosen one is captured.
    fs::write(saves.path().join("save.dat"), "local progress").unwrap();
    fs::write(control.path().join("save.dat"), "two").unwrap();
    let (core, rx) = open();
    assert!(wait_for(&rx, |n| matches!(
        n,
        Notification::VersionCreated { game_id, .. } if *game_id == control_game.id
    )));
    std::thread::sleep(Duration::from_secs(1));
    let imported = core
        .get_games()
        .unwrap()
        .into_iter()
        .find(|g| g.game.title == "Imported")
        .unwrap();
    assert_eq!(imported.versions.len(), 1);
    assert!(imported.last_version.is_none());
}

/// A core watching with settings below the UI's bounds, so tests run in seconds.
fn open_watched(
    data: &std::path::Path,
) -> (WorldlineCore, std::sync::mpsc::Receiver<Notification>) {
    worldline_core::store::write_json(
        &data.join("monitoring_settings.json"),
        &MonitoringSettings {
            version_cooldown_minutes: 0,
            snapshot_debounce_seconds: 0.2,
            ..MonitoringSettings::default()
        },
    )
    .unwrap();
    WorldlineCore::open(
        data,
        OpenOptions {
            watch: true,
            debounce_seconds: None,
        },
    )
    .unwrap()
}

/// Writes `contents` until the watcher captures it; returns the notification's
/// version number and branch. Retrying covers the folder not being watched yet.
fn capture(
    rx: &std::sync::mpsc::Receiver<Notification>,
    save: &std::path::Path,
    contents: &str,
) -> (i32, Option<BranchFrom>) {
    rx.try_iter().for_each(drop);
    for _ in 0..20 {
        fs::write(save, contents).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(left) {
                Ok(Notification::VersionCreated {
                    version_number,
                    branch_from,
                    ..
                }) => return (version_number, branch_from),
                Ok(_) => {}
                Err(_) => break,
            }
        }
    }
    panic!("{contents:?} was not captured");
}

#[test]
fn the_first_capture_after_a_restore_reports_the_branch_it_starts() {
    let data = tempfile::tempdir().unwrap();
    let saves = tempfile::tempdir().unwrap();
    let save = saves.path().join("save.dat");
    let (core, rx) = open_watched(data.path());
    fs::write(&save, "first").unwrap();
    let game = core
        .save_game(
            GameForm {
                id: None,
                title: "Branching".into(),
            },
            Some(vec![saves.path().to_string_lossy().into_owned()]),
        )
        .unwrap();
    let first = core.get_game(game.id).unwrap().last_version.unwrap();
    core.tag_version(first.id, "Before the boss".into())
        .unwrap();

    let (second, branch) = capture(&rx, &save, "second");
    assert_eq!(branch, None, "a capture on the current line");

    core.restore_version(first.id).unwrap();
    let (forked, branch) = capture(&rx, &save, "another way");
    assert!(forked > second);
    assert_eq!(
        branch,
        Some(BranchFrom {
            version_number: first.version_number,
            tag: Some("Before the boss".into()),
        })
    );

    let (next, branch) = capture(&rx, &save, "further");
    assert!(next > forked);
    assert_eq!(branch, None, "the new line continues");
}

#[test]
fn a_capture_that_cannot_be_written_reports_checkpoint_failed() {
    let data = tempfile::tempdir().unwrap();
    let saves = tempfile::tempdir().unwrap();
    let (core, rx) = open_watched(data.path());
    fs::write(saves.path().join("save.dat"), "first").unwrap();
    let game = core
        .save_game(
            GameForm {
                id: None,
                title: "Unwritable".into(),
            },
            Some(vec![saves.path().to_string_lossy().into_owned()]),
        )
        .unwrap();
    // A file where the new content's chunk folder goes makes its write fail.
    use sha2::Digest as _;
    let hash = worldline_core::store::format_hash(&sha2::Sha256::digest(b"blocked"));
    let digest = worldline_core::store::parse_hash(&hash).unwrap();
    let chunks = data.path().join("repository").join("chunks");
    fs::create_dir_all(&chunks).unwrap();
    fs::write(chunks.join(&digest[..2]), "not a folder").unwrap();

    let mut error_first = false;
    let mut failed = None;
    for _ in 0..20 {
        fs::write(saves.path().join("save.dat"), "blocked").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(left) {
                Ok(Notification::Error(message)) => {
                    error_first = message.starts_with("Automatic checkpoint failed")
                }
                Ok(Notification::CheckpointFailed { game_id, reason }) => {
                    failed = Some((game_id, reason));
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        if failed.is_some() {
            break;
        }
    }
    assert_eq!(failed, Some((game.id, CaptureFailure::Other)));
    assert!(error_first, "the existing error is still sent");
    assert_eq!(core.get_game(game.id).unwrap().versions.len(), 1);
    // The save it couldn't record shows as unrecorded until the retry.
    assert!(matches!(unrecorded(&rx, game.id), Some(Some(_))));
}

/// A core watching with a five-minute cooldown, so a change waits unrecorded.
fn open_cooling(
    data: &std::path::Path,
) -> (WorldlineCore, std::sync::mpsc::Receiver<Notification>) {
    worldline_core::store::write_json(
        &data.join("monitoring_settings.json"),
        &MonitoringSettings {
            version_cooldown_minutes: 5,
            snapshot_debounce_seconds: 0.2,
            ..MonitoringSettings::default()
        },
    )
    .unwrap();
    WorldlineCore::open(
        data,
        OpenOptions {
            watch: true,
            debounce_seconds: None,
        },
    )
    .unwrap()
}

/// The `due_at` of the game's next `UnrecordedChanges`.
fn unrecorded(
    rx: &std::sync::mpsc::Receiver<Notification>,
    game: i32,
) -> Option<Option<NaiveDateTime>> {
    let deadline = Instant::now() + Duration::from_secs(20);
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(Notification::UnrecordedChanges { game_id, due_at }) if game_id == game => {
                return Some(due_at);
            }
            Ok(_) => {}
            Err(_) => return None,
        }
    }
    None
}

/// Writes `contents` until the watcher reports it unrecorded; returns its `due_at`.
/// Retrying covers the folder not being watched yet.
fn change(
    rx: &std::sync::mpsc::Receiver<Notification>,
    game: i32,
    save: &std::path::Path,
    contents: &str,
) -> NaiveDateTime {
    for _ in 0..20 {
        fs::write(save, contents).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(left) {
                Ok(Notification::UnrecordedChanges {
                    game_id,
                    due_at: Some(due_at),
                }) if game_id == game => return due_at,
                Ok(_) => {}
                Err(_) => break,
            }
        }
    }
    panic!("{contents:?} was not reported unrecorded");
}

fn add_watched_game(core: &WorldlineCore, saves: &std::path::Path) -> i32 {
    core.save_game(
        GameForm {
            id: None,
            title: "Silksong".into(),
        },
        Some(vec![saves.to_string_lossy().into_owned()]),
    )
    .unwrap()
    .id
}

#[test]
fn a_change_inside_the_cooldown_is_unrecorded_until_captured() {
    let data = tempfile::tempdir().unwrap();
    let saves = tempfile::tempdir().unwrap();
    let save = saves.path().join("save.dat");
    let (core, rx) = open_cooling(data.path());
    fs::write(&save, "before the death").unwrap();
    let game = add_watched_game(&core, saves.path());
    let first = core.get_game(game).unwrap().last_version.unwrap();

    let due_at = change(&rx, game, &save, "after the death");
    assert_eq!(due_at, first.created_at + chrono::Duration::minutes(5));
    assert_eq!(core.unrecorded_changes(), vec![(game, due_at)]);

    // The flush captures it, as the end of the cooldown would.
    core.shutdown();
    assert_eq!(unrecorded(&rx, game), Some(None));
    assert!(core.unrecorded_changes().is_empty());
    assert_eq!(core.get_game(game).unwrap().versions.len(), 2);
}

#[test]
fn a_new_cooldown_moves_when_unrecorded_changes_are_due() {
    let data = tempfile::tempdir().unwrap();
    let saves = tempfile::tempdir().unwrap();
    let save = saves.path().join("save.dat");
    let (core, rx) = open_cooling(data.path());
    fs::write(&save, "before the death").unwrap();
    let game = add_watched_game(&core, saves.path());
    let first = core.get_game(game).unwrap().last_version.unwrap();
    change(&rx, game, &save, "after the death");

    core.update_monitoring_settings(MonitoringSettings {
        version_cooldown_minutes: 30,
        snapshot_debounce_seconds: 1.0,
        ..MonitoringSettings::default()
    })
    .unwrap();
    let due_at = first.created_at + chrono::Duration::minutes(30);
    assert_eq!(unrecorded(&rx, game), Some(Some(due_at)));
    assert_eq!(core.unrecorded_changes(), vec![(game, due_at)]);
}

#[test]
fn the_same_bytes_written_again_are_not_unrecorded() {
    let data = tempfile::tempdir().unwrap();
    let saves = tempfile::tempdir().unwrap();
    let save = saves.path().join("save.dat");
    let (core, rx) = open_cooling(data.path());
    fs::write(&save, "same").unwrap();
    let game = add_watched_game(&core, saves.path());

    let mut seen = false;
    for _ in 0..20 {
        fs::write(&save, "same").unwrap();
        if wait_for(
            &rx,
            |n| matches!(n, Notification::ChangeDetected { game_id } if *game_id == game),
        ) {
            seen = true;
            break;
        }
    }
    assert!(seen, "the rewrite was not seen");
    // The check runs right after the change is reported.
    std::thread::sleep(Duration::from_secs(1));
    assert!(
        !rx.try_iter()
            .any(|n| matches!(n, Notification::UnrecordedChanges { .. }))
    );
    assert!(core.unrecorded_changes().is_empty());

    change(&rx, game, &save, "different");
}

#[test]
fn a_change_made_while_the_app_was_closed_is_unrecorded_on_start() {
    let data = tempfile::tempdir().unwrap();
    let saves = tempfile::tempdir().unwrap();
    let save = saves.path().join("save.dat");
    let (core, _) = open_cooling(data.path());
    fs::write(&save, "before").unwrap();
    let game = add_watched_game(&core, saves.path());
    let first = core.get_game(game).unwrap().last_version.unwrap();
    drop(core);

    fs::write(&save, "while closed").unwrap();
    let (core, rx) = open_cooling(data.path());
    let due_at = first.created_at + chrono::Duration::minutes(5);
    assert_eq!(unrecorded(&rx, game), Some(Some(due_at)));
    assert_eq!(core.unrecorded_changes(), vec![(game, due_at)]);
}

#[test]
fn checkpoint_keep_and_restore_clear_unrecorded_changes() {
    let data = tempfile::tempdir().unwrap();
    let saves = tempfile::tempdir().unwrap();
    let save = saves.path().join("save.dat");
    let (core, rx) = open_cooling(data.path());
    fs::write(&save, "first").unwrap();
    let game = add_watched_game(&core, saves.path());
    let first = core.get_game(game).unwrap().last_version.unwrap();

    change(&rx, game, &save, "second");
    core.checkpoint(game).unwrap();
    assert_eq!(unrecorded(&rx, game), Some(None));
    assert!(core.unrecorded_changes().is_empty());

    change(&rx, game, &save, "third");
    core.keep(game, "Kept in game, 21:04", || true).unwrap();
    assert_eq!(unrecorded(&rx, game), Some(None));
    assert!(core.unrecorded_changes().is_empty());

    change(&rx, game, &save, "fourth");
    core.restore_version(first.id).unwrap();
    assert_eq!(unrecorded(&rx, game), Some(None));
    assert!(core.unrecorded_changes().is_empty());
}
