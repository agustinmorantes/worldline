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
