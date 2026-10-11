//! A game that can't be added says why (ADR 0007), and leaves nothing behind
//! that would block trying again.

mod common;

use common::Device;
use worldline_core::error::{ErrorKind, error_kind};
use worldline_core::models::GameForm;

fn add(device: &Device, title: &str, dir: &std::path::Path) -> anyhow::Result<()> {
    device
        .core
        .save_game(
            GameForm {
                id: None,
                title: title.into(),
            },
            Some(vec![dir.to_string_lossy().into_owned()]),
        )
        .map(drop)
}

#[test]
fn a_taken_name_or_folder_says_so() {
    let device = Device::new();
    device.write("save.dat", "v1");
    device.add_game("Taken");
    let other = tempfile::tempdir().unwrap();

    let error = add(&device, "taken", other.path()).unwrap_err();
    assert_eq!(error_kind(&error), Some(ErrorKind::NameTaken));

    let inside = device.save_dir().join("slot1");
    std::fs::create_dir_all(&inside).unwrap();
    let error = add(&device, "Elsewhere", &inside).unwrap_err();
    assert_eq!(error_kind(&error), Some(ErrorKind::FolderTaken));
}

#[cfg(windows)]
#[test]
fn saves_the_game_holds_fail_the_add_without_keeping_the_game() {
    use std::os::windows::fs::OpenOptionsExt;

    let device = Device::new();
    device.write("save.dat", "v1");
    let held = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0)
        .open(device.save_dir().join("save.dat"))
        .unwrap();

    let error = add(&device, "Running", &device.save_dir()).unwrap_err();
    assert_eq!(error_kind(&error), Some(ErrorKind::SavesInUse), "{error:#}");
    assert!(device.core.get_games().unwrap().is_empty());

    drop(held);
    add(&device, "Running", &device.save_dir()).expect("trying again works");
    assert_eq!(device.core.get_games().unwrap().len(), 1);
}

#[test]
fn far_more_files_than_saves_take_need_the_users_word() {
    let device = Device::new();
    let many = worldline_core::history::MIN_CAPTURE_LIMIT.files + 1;
    std::fs::create_dir_all(device.save_dir()).unwrap();
    for n in 0..many {
        std::fs::write(device.save_dir().join(format!("{n}.dat")), "").unwrap();
    }
    let form = || GameForm {
        id: None,
        title: "Huge".into(),
    };
    let dirs = || Some(vec![device.save_dir().to_string_lossy().into_owned()]);

    let error = device.core.save_game(form(), dirs()).unwrap_err();
    assert_eq!(error_kind(&error), Some(ErrorKind::TooLarge), "{error:#}");
    assert!(device.core.get_games().unwrap().is_empty());

    let game = device
        .core
        .save_large_game(form(), dirs())
        .expect("the user insisted");
    let game = device.core.get_game(game.id).unwrap();
    assert_eq!(game.versions.len(), 1, "its saves were captured");
}
