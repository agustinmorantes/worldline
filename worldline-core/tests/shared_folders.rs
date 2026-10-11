//! Folders shared by many apps (the home folder, AppData, Documents and
//! their ancestors) are never tracked: capturing, watching or restoring one
//! would touch far more than a game's saves.

mod common;

use chrono::Utc;
use common::Device;
use std::path::PathBuf;
use uuid::Uuid;
use worldline_core::models::GameForm;
use worldline_core::store::{Store, TrackedDirMeta};

fn home() -> PathBuf {
    dirs::home_dir().expect("a home folder")
}

#[test]
fn a_shared_folder_cannot_be_tracked() {
    let device = Device::new();
    let mut shared = vec![home(), home().parent().unwrap().to_path_buf()];
    if let Ok(appdata) = std::env::var("APPDATA") {
        shared.push(appdata.into());
    }
    for dir in shared {
        let added = device.core.save_game(
            GameForm {
                id: None,
                title: "Too Broad".into(),
            },
            Some(vec![dir.to_string_lossy().into_owned()]),
        );
        let error = added.expect_err("a shared folder is refused").to_string();
        assert!(error.contains("other apps"), "{error}");
    }
    assert!(device.core.get_games().unwrap().is_empty());
}

#[test]
fn a_shared_folder_from_another_device_is_ignored_here() {
    let device = Device::new();
    device.write("save.dat", "v1");
    let game = device.add_game("Pulled");
    // Another device (or an older version) wrote `<home>` as a tracked folder.
    let store = Store::open_existing(device.core.repository_root()).unwrap();
    let record = store.catalog().unwrap().pop().unwrap();
    let uuid = Uuid::new_v4();
    let now = Utc::now();
    store
        .write_tracked_dir(
            &record.meta.folder_slug,
            &TrackedDirMeta {
                uuid,
                game_uuid: record.meta.uuid,
                folder_slug: format!("home__d_{}", &uuid.simple().to_string()[..8]),
                path: "<home>".into(),
                label: None,
                exclusions: Vec::new(),
                retired_at: None,
                created_at: now,
                updated_at: now,
            },
        )
        .unwrap();
    device.core.reindex().unwrap();

    let dirs: Vec<PathBuf> = device
        .game(game.game.id)
        .tracked_dirs
        .iter()
        .map(|dir| PathBuf::from(&dir.path))
        .collect();
    assert_eq!(dirs, [device.save_dir()]);

    // Capture and restore still work on the game's own folder.
    let first = device.active_version(game.game.id);
    device.write("save.dat", "v2");
    device.checkpoint(game.game.id);
    device.core.restore_version(first.id).unwrap();
    assert_eq!(device.read("save.dat").as_deref(), Some("v1"));
}
