//! The one end-to-end test through real filesystem notifications. Scheduling
//! rules are unit-tested with a fake clock in `detection::scheduler`.

mod common;

use std::fs;
use std::time::{Duration, Instant};
use worldline_core::detection::Notification;
use worldline_core::models::{GameForm, MonitoringSettings};
use worldline_core::{OpenOptions, TimelineCore};

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
        },
    )
    .unwrap();
    let (core, rx) = TimelineCore::open(
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
    let bundle = out.path().join("bundle");
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
        },
    )
    .unwrap();
    let open = || {
        TimelineCore::open(
            data.path(),
            OpenOptions {
                watch: true,
                debounce_seconds: None,
            },
        )
        .unwrap()
    };
    let (core, _) = open();
    let summary = TimelineCore::inspect_history_bundle(&bundle).unwrap();
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
