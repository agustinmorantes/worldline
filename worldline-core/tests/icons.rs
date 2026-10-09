//! Game icons and accent hues live in the store and travel with the game
//! (ADR 0005).

mod common;

use common::{Device, edit_bundle};
use image::{Rgba, RgbaImage};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;
use worldline_core::icon::fallback_hue;
use worldline_core::models::Game;
use worldline_core::store::Store;

const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const GRAY: [u8; 4] = [128, 128, 128, 255];

/// A solid-color image file the user could pick.
fn image_file(dir: &Path, color: [u8; 4]) -> String {
    let path = dir.join(format!("{}.png", color.map(|c| c.to_string()).join("-")));
    RgbaImage::from_pixel(48, 48, Rgba(color))
        .save(&path)
        .unwrap();
    path.to_string_lossy().into_owned()
}

fn set_icon(device: &Device, id: i32, color: [u8; 4]) {
    let path = image_file(device.saves.path(), color);
    device.core.set_game_icon(id, &path).unwrap();
}

fn game(device: &Device, uuid: &str) -> Game {
    device
        .core
        .get_games()
        .unwrap()
        .into_iter()
        .find(|g| g.game.uuid == uuid)
        .expect("game is indexed")
        .game
}

fn remote_icon(remote: &Path, game: &Game) -> PathBuf {
    Store::open_existing(remote)
        .unwrap()
        .icon_path(&game.folder_slug)
}

fn icon_bytes(game: &Game) -> Vec<u8> {
    fs::read(game.icon_path.as_ref().expect("an icon")).unwrap()
}

#[test]
fn an_icon_and_its_hue_reach_the_other_device() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    a.write("save.dat", "1");
    let id = a.add_game("Painted").game.id;
    set_icon(&a, id, RED);
    let on_a = a.game(id).game;
    assert_eq!(
        (on_a.accent_hue, on_a.accent_hue_source.as_str()),
        (0, "auto")
    );
    let png = image::open(on_a.icon_path.as_ref().unwrap()).unwrap();
    assert_eq!((png.width(), png.height()), (256, 256), "normalized");
    a.core.refresh_sync().unwrap();
    assert!(remote_icon(remote.path(), &on_a).is_file());

    let b = Device::synced(remote.path());
    b.core
        .adopt_remote_game(Uuid::parse_str(&on_a.uuid).unwrap())
        .unwrap();
    let on_b = game(&b, &on_a.uuid);
    assert_eq!(icon_bytes(&on_b), icon_bytes(&on_a));
    assert_eq!(
        (on_b.accent_hue, on_b.accent_hue_source.as_str()),
        (0, "auto")
    );
    // Reindexing restores the icon path: it is projected from the files.
    b.core.reindex().unwrap();
    assert_eq!(game(&b, &on_a.uuid).icon_path, on_b.icon_path);
    assert!(b.core.check_integrity(true).unwrap().is_ok());
    assert!(a.core.check_integrity(true).unwrap().is_ok());
}

#[test]
fn concurrent_icon_and_hue_edits_resolve_last_writer_wins_on_both_devices() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    a.write("save.dat", "1");
    let id_a = a.add_game("Contested").game.id;
    a.core.refresh_sync().unwrap();
    let uuid = a.game(id_a).game.uuid;
    let b = Device::synced(remote.path());
    let id_b = b
        .core
        .adopt_remote_game(Uuid::parse_str(&uuid).unwrap())
        .unwrap()
        .game
        .id;

    // Offline on both; B's edit is the newer one but A syncs last.
    set_icon(&a, id_a, BLUE);
    set_icon(&b, id_b, GREEN);
    b.core.refresh_sync().unwrap();
    a.core.refresh_sync().unwrap();
    b.core.refresh_sync().unwrap();
    for (device, id) in [(&a, id_a), (&b, id_b)] {
        let game = device.game(id).game;
        assert_eq!(game.accent_hue, 120, "the newer (green) icon wins");
        let decoded = image::load_from_memory(&icon_bytes(&game)).unwrap();
        assert_eq!(decoded.to_rgba8().get_pixel(0, 0).0, GREEN);
    }
    let remote_png = fs::read(remote_icon(remote.path(), &a.game(id_a).game)).unwrap();
    assert_eq!(remote_png, icon_bytes(&a.game(id_a).game));

    // The same for user colors: the newer pick wins, whoever syncs first.
    b.core.set_game_accent(id_b, Some(10)).unwrap();
    a.core.set_game_accent(id_a, Some(200)).unwrap();
    a.core.refresh_sync().unwrap();
    b.core.refresh_sync().unwrap();
    a.core.refresh_sync().unwrap();
    for (device, id) in [(&a, id_a), (&b, id_b)] {
        let game = device.game(id).game;
        assert_eq!(
            (game.accent_hue, game.accent_hue_source.as_str()),
            (200, "user")
        );
        assert_eq!(
            image::load_from_memory(&icon_bytes(&game))
                .unwrap()
                .to_rgba8()
                .get_pixel(0, 0)
                .0,
            GREEN,
            "the icon is unchanged"
        );
    }
}

#[test]
fn a_user_hue_survives_icon_changes_and_none_returns_to_auto() {
    let device = Device::new();
    device.write("save.dat", "1");
    let id = device.add_game("Tinted").game.id;
    let fallback = fallback_hue(Uuid::parse_str(&device.game(id).game.uuid).unwrap());
    let shown = |device: &Device| {
        let game = device.game(id).game;
        (game.accent_hue, game.accent_hue_source)
    };
    assert_eq!(shown(&device), (i32::from(fallback), "auto".into()));
    assert_eq!(device.game(id).game.icon_path, None);

    assert!(device.core.set_game_accent(id, Some(360)).is_err());
    device.core.set_game_accent(id, Some(300)).unwrap();
    set_icon(&device, id, RED);
    assert_eq!(shown(&device), (300, "user".into()));

    device.core.set_game_accent(id, None).unwrap();
    assert_eq!(shown(&device), (0, "auto".into()));
    set_icon(&device, id, BLUE);
    assert_eq!(shown(&device), (240, "auto".into()));
    set_icon(&device, id, GRAY);
    assert_eq!(
        shown(&device),
        (i32::from(fallback), "auto".into()),
        "a gray icon falls back to the UUID hue"
    );
}

#[test]
fn bundles_carry_the_icon() {
    let a = Device::new();
    a.write("save.dat", "1");
    let id = a.add_game("Bundled").game.id;
    set_icon(&a, id, BLUE);
    a.core.set_game_accent(id, Some(42)).unwrap();
    let out = tempfile::tempdir().unwrap();
    let bundle = out.path().join("bundle.zip");
    a.core.export_game_history(id, &bundle).unwrap();

    let b = Device::new();
    let summary = worldline_core::WorldlineCore::inspect_history_bundle(&bundle).unwrap();
    let mappings: HashMap<_, _> = summary
        .active_tracked_dirs
        .iter()
        .map(|dir| (dir.uuid, b.save_dir()))
        .collect();
    b.core.import_game_history(&bundle, &mappings).unwrap();
    let imported = b.game_by_title("Bundled").unwrap().game;
    assert_eq!(icon_bytes(&imported), icon_bytes(&a.game(id).game));
    assert_eq!(
        (imported.accent_hue, imported.accent_hue_source.as_str()),
        (42, "user")
    );

    // A bundle whose icon is missing is incomplete.
    edit_bundle(&bundle, |dir| {
        fs::remove_file(
            Store::open_existing(dir)
                .unwrap()
                .icon_path(&imported.folder_slug),
        )
        .unwrap()
    });
    let c = Device::new();
    assert!(c.core.import_game_history(&bundle, &mappings).is_err());
}

#[test]
fn linking_keeps_the_remote_icon_and_falls_back_to_the_local_one() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    a.write("save.dat", "a");
    let with_icon = a.add_game("Iconic").game.id;
    set_icon(&a, with_icon, RED);
    let saves = tempfile::tempdir().unwrap();
    let bare = a.add_game_at("Bare", saves.path()).game.id;
    a.core.refresh_sync().unwrap();

    // B tracked both before sync, with icons of its own (as the install
    // scanner sets them).
    let b = Device::new();
    let local = b.add_game_at("Iconic", &a.save_dir()).game.id;
    set_icon(&b, local, BLUE);
    let local_bare = b.add_game_at("Bare", saves.path()).game.id;
    set_icon(&b, local_bare, GREEN);
    b.core
        .update_sync_settings(worldline_core::sync::SyncSettings {
            remote_path: Some(remote.path().to_string_lossy().into_owned()),
        })
        .unwrap();
    let report = b.core.refresh_sync().unwrap();
    assert!(report.problems.is_empty(), "{:?}", report.problems);

    let iconic = b.game_by_title("Iconic").unwrap().game;
    assert_eq!(iconic.uuid, a.game(with_icon).game.uuid);
    assert_eq!(icon_bytes(&iconic), icon_bytes(&a.game(with_icon).game));
    assert_eq!(iconic.accent_hue, 0, "the remote icon wins");

    let linked_bare = b.game_by_title("Bare").unwrap().game;
    assert_eq!(linked_bare.uuid, a.game(bare).game.uuid);
    assert_eq!(linked_bare.accent_hue, 120, "the local icon fills the gap");
    a.core.refresh_sync().unwrap();
    let bare_on_a = a.game(bare).game;
    assert_eq!(bare_on_a.accent_hue, 120);
    assert_eq!(icon_bytes(&bare_on_a), icon_bytes(&linked_bare));
}

#[test]
fn an_icon_that_has_not_arrived_does_not_block_sync_and_is_healed() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    a.write("save.dat", "1");
    let id = a.add_game("Late").game.id;
    set_icon(&a, id, BLUE);
    a.core.refresh_sync().unwrap();
    let on_a = a.game(id).game;
    fs::remove_file(remote_icon(remote.path(), &on_a)).unwrap();

    let b = Device::synced(remote.path());
    b.core
        .adopt_remote_game(Uuid::parse_str(&on_a.uuid).unwrap())
        .unwrap();
    let on_b = game(&b, &on_a.uuid);
    assert_eq!(on_b.icon_path, None, "not arrived");
    assert_eq!(on_b.accent_hue, 240, "the hue travels with the metadata");

    a.core.refresh_sync().unwrap();
    assert!(remote_icon(remote.path(), &on_a).is_file(), "republished");
    let report = b.core.refresh_sync().unwrap();
    assert!(report.problems.is_empty(), "{:?}", report.problems);
    assert_eq!(icon_bytes(&game(&b, &on_a.uuid)), icon_bytes(&on_a));
}
