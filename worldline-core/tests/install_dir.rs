//! Install dirs: device-local, filled by the install scan, and the base of
//! game-running detection.

mod common;

use common::Device;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use worldline_core::detection::{Notification, NotificationReceiver};
use worldline_core::error::{ErrorKind, error_kind};
use worldline_core::models::GameForm;
use worldline_core::{OpenOptions, WorldlineCore};

const TITLE: &str = "Ember Trail";

/// Makes the install scanner find `TITLE` installed under a new scan root,
/// with its saves in `<install dir>/saves`. Returns the root.
fn install(core: &WorldlineCore) -> tempfile::TempDir {
    let cache = core.data_dir.join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(
        cache.join("manifest.yaml"),
        format!("{TITLE}:\n  files:\n    <base>/saves:\n      tags: [save]\n"),
    )
    .unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(saves(root.path())).unwrap();
    fs::write(saves(root.path()).join("x"), "x").unwrap();
    core.add_scan_root(&root.path().to_string_lossy()).unwrap();
    root
}

fn install_dir(root: &Path) -> PathBuf {
    root.join(TITLE)
}

fn saves(root: &Path) -> PathBuf {
    install_dir(root).join("saves")
}

fn add_game(core: &WorldlineCore, title: &str, dir: &Path) -> i32 {
    core.save_game(
        GameForm {
            id: None,
            title: title.into(),
        },
        Some(vec![dir.to_string_lossy().into_owned()]),
    )
    .unwrap()
    .id
}

fn stored(core: &WorldlineCore, id: i32) -> Option<String> {
    core.get_game(id).unwrap().game.install_dir
}

fn as_string(path: PathBuf) -> Option<String> {
    Some(path.to_string_lossy().into_owned())
}

#[test]
fn the_scan_fills_the_install_dir_and_reindex_keeps_it() {
    let device = Device::new();
    let core = &device.core;
    let root = install(core);
    assert!(
        core.scan_installed()
            .unwrap()
            .iter()
            .any(|item| item.title == TITLE)
    );

    let id = add_game(core, TITLE, &saves(root.path()));
    assert_eq!(stored(core, id), as_string(install_dir(root.path())));
    let manual = device.add_game("Hand Added");
    assert_eq!(manual.game.install_dir, None, "no scan found it");

    core.reindex().unwrap();
    assert_eq!(stored(core, id), as_string(install_dir(root.path())));
}

#[test]
fn a_moved_game_follows_and_an_uninstalled_one_loses_its_dir() {
    let device = Device::new();
    let core = &device.core;
    let old_root = install(core);
    core.scan_installed().unwrap();
    let id = add_game(core, TITLE, &saves(old_root.path()));

    // Moved to another library.
    let new_root = tempfile::tempdir().unwrap();
    fs::rename(install_dir(old_root.path()), install_dir(new_root.path())).unwrap();
    core.remove_scan_root(&old_root.path().to_string_lossy())
        .unwrap();
    core.add_scan_root(&new_root.path().to_string_lossy())
        .unwrap();
    core.scan_installed().unwrap();
    assert_eq!(stored(core, id), as_string(install_dir(new_root.path())));

    // A scan that misses it keeps a dir that still exists.
    core.remove_scan_root(&new_root.path().to_string_lossy())
        .unwrap();
    core.scan_installed().unwrap();
    assert_eq!(stored(core, id), as_string(install_dir(new_root.path())));

    // Uninstalled.
    fs::remove_dir_all(install_dir(new_root.path())).unwrap();
    core.scan_installed().unwrap();
    assert_eq!(stored(core, id), None);
}

/// A long-running process started from an executable copied into `dir`.
fn run_from(dir: &Path) -> Child {
    let (source, name, args): (PathBuf, &str, &[&str]) = if cfg!(windows) {
        let system = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        (
            Path::new(&system).join("System32").join("PING.EXE"),
            "game.exe",
            &["-n", "120", "127.0.0.1"],
        )
    } else {
        (PathBuf::from("/bin/sleep"), "game", &["120"])
    };
    let exe = dir.join("bin").join(name);
    fs::create_dir_all(exe.parent().unwrap()).unwrap();
    fs::copy(source, &exe).unwrap();
    Command::new(&exe)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

fn wait_for(notifications: &NotificationReceiver, wanted: &Notification) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match notifications.recv_timeout(left) {
            Ok(notification) if &notification == wanted => return,
            Ok(_) => {}
            Err(_) => break,
        }
    }
    panic!("no {wanted:?}");
}

#[test]
fn running_games_are_reported_as_they_start_and_stop() {
    let data = tempfile::tempdir().unwrap();
    let (core, notifications) = WorldlineCore::open_shared(
        data.path(),
        OpenOptions {
            watch: true,
            debounce_seconds: None,
        },
    )
    .unwrap();
    let root = install(&core);
    core.scan_installed().unwrap();
    let game_id = add_game(&core, TITLE, &saves(root.path()));
    assert!(core.running_games().is_empty());

    let mut game = run_from(&install_dir(root.path()));
    wait_for(&notifications, &Notification::GameStarted { game_id });
    assert_eq!(core.running_games(), vec![game_id]);

    game.kill().unwrap();
    game.wait().unwrap();
    wait_for(&notifications, &Notification::GameStopped { game_id });
    assert!(core.running_games().is_empty());
    core.shutdown();
}

#[test]
fn a_running_game_refuses_a_rewind_before_touching_anything() {
    let device = Device::new();
    let root = install(&device.core);
    device.core.scan_installed().unwrap();
    let id = add_game(&device.core, TITLE, &saves(root.path()));
    let first = device.active_version(id);
    fs::write(saves(root.path()).join("x"), "later").unwrap();
    device.checkpoint(id);
    let versions = device.game(id).versions.len();

    let mut game = run_from(&install_dir(root.path()));
    let error = device.core.restore_version(first.id).unwrap_err();
    game.kill().unwrap();
    game.wait().unwrap();

    assert_eq!(error_kind(&error), Some(ErrorKind::GameRunning));
    assert_eq!(
        device.game(id).versions.len(),
        versions,
        "no checkpoint first"
    );
    assert_eq!(
        fs::read_to_string(saves(root.path()).join("x")).unwrap(),
        "later"
    );
}

/// Sync applies another device's progress by itself only when it sees the
/// game closed, which needs the install dir.
#[test]
fn an_emptied_tip_from_another_device_is_not_applied_over_saves() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    let root = install(&a.core);
    a.core.scan_installed().unwrap();
    let id_a = add_game(&a.core, TITLE, &saves(root.path()));
    a.core.refresh_sync().unwrap();
    let b = Device::synced(remote.path());
    let cache = b.core.data_dir.join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::copy(
        a.core.data_dir.join("cache").join("manifest.yaml"),
        cache.join("manifest.yaml"),
    )
    .unwrap();
    b.core
        .add_scan_root(&root.path().to_string_lossy())
        .unwrap();
    b.core.scan_installed().unwrap();
    let uuid = uuid::Uuid::parse_str(&a.game(id_a).game.uuid).unwrap();
    let id_b = b.core.adopt_remote_game(uuid).unwrap().game.id;
    assert!(b.game(id_b).game.install_dir.is_some());
    let before = b.active_version(id_b);

    // A's saves are deleted, and A records that.
    fs::remove_file(saves(root.path()).join("x")).unwrap();
    a.checkpoint(id_a);
    a.core.push_sync().unwrap();
    fs::write(saves(root.path()).join("x"), "x").unwrap(); // B's machine still has it

    b.core.refresh_sync().unwrap();

    assert_eq!(b.active_version(id_b).id, before.id);
    assert_eq!(
        fs::read_to_string(saves(root.path()).join("x")).unwrap(),
        "x"
    );
}

#[test]
fn at_startup_a_game_without_an_icon_gets_its_installs_in_the_background() {
    let data = tempfile::tempdir().unwrap();
    let root;
    let id;
    {
        let (core, _) = WorldlineCore::open(data.path(), OpenOptions::default()).unwrap();
        root = install(&core);
        id = add_game(&core, TITLE, &saves(root.path()));
        assert_eq!(core.get_game(id).unwrap().game.icon_path, None);
    }
    // Installed with an icon after it was added; nothing scanned since.
    image::RgbaImage::from_pixel(48, 48, image::Rgba([0, 0, 255, 255]))
        .save_with_format(
            install_dir(root.path()).join("game.ico"),
            image::ImageFormat::Png,
        )
        .unwrap();

    let (core, notifications) = WorldlineCore::open_shared(
        data.path(),
        OpenOptions {
            watch: true,
            debounce_seconds: None,
        },
    )
    .unwrap();
    wait_for(&notifications, &Notification::GamesChanged);
    assert!(core.get_game(id).unwrap().game.icon_path.is_some());
    core.shutdown();
}
