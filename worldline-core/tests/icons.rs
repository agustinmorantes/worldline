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
use worldline_core::store::{GameMeta, IconSource, Store};
use worldline_core::{OpenOptions, WorldlineCore};

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
    assert_eq!((png.width(), png.height()), (48, 48), "not upscaled");
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
    let picked = image_file(a.saves.path(), BLUE);
    a.core
        .set_game_icon_as(id, &picked, IconSource::Auto)
        .unwrap();
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
    assert_eq!(meta(&b, &imported.uuid).icon_source, IconSource::Auto);

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

// ---- icons filled from installs (phase 2 of game icons) ----------------------------

const INSTALLED: &str = "Ember Trail";

/// Puts a solid `color` `.ico` of `size` px in `INSTALLED`'s install folder.
fn install_icon(root: &tempfile::TempDir, color: [u8; 4], size: u32) {
    // Some `.ico` files are really PNGs; they decode all the same.
    RgbaImage::from_pixel(size, size, Rgba(color))
        .save_with_format(
            root.path().join(INSTALLED).join("game.ico"),
            image::ImageFormat::Png,
        )
        .unwrap();
}

/// The game's stored metadata, as sync and bundles carry it.
fn meta(device: &Device, uuid: &str) -> GameMeta {
    Store::open_existing(device.core.repository_root())
        .unwrap()
        .find_game(Uuid::parse_str(uuid).unwrap())
        .unwrap()
        .expect("stored")
}

fn icon_size(device: &Device, uuid: &str) -> (u32, u32) {
    let png = image::open(game(device, uuid).icon_path.expect("an icon")).unwrap();
    (png.width(), png.height())
}

/// Makes the install scan find `INSTALLED` under a new scan root, with its
/// saves in `<install>/saves` and a solid `color` `.ico` in its folder.
fn install(device: &Device, color: [u8; 4]) -> tempfile::TempDir {
    fs::write(
        device.core.data_dir.join("cache").join("manifest.yaml"),
        format!("{INSTALLED}:\n  files:\n    <base>/saves:\n      tags: [save]\n"),
    )
    .unwrap();
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join(INSTALLED);
    fs::create_dir_all(dir.join("saves")).unwrap();
    fs::write(dir.join("saves").join("x"), "x").unwrap();
    install_icon(&root, color, 48);
    device
        .core
        .add_scan_root(&root.path().to_string_lossy())
        .unwrap();
    root
}

fn install_saves(root: &tempfile::TempDir) -> PathBuf {
    root.path().join(INSTALLED).join("saves")
}

/// The color of the game's stored icon, `None` without one.
fn icon_color(device: &Device, uuid: &str) -> Option<[u8; 4]> {
    let path = game(device, uuid).icon_path?;
    Some(image::open(path).unwrap().to_rgba8().get_pixel(0, 0).0)
}

#[test]
fn a_game_without_an_icon_gets_its_installs_after_the_scan() {
    let device = Device::new();
    let root = install(&device, RED);
    let uuid = device
        .add_game_at(INSTALLED, &install_saves(&root))
        .game
        .uuid;
    assert_eq!(icon_color(&device, &uuid), None, "added without one");

    device.core.scan_installed().unwrap();
    assert_eq!(icon_color(&device, &uuid), Some(RED));
}

#[test]
fn a_user_icon_is_not_replaced_by_the_installs() {
    let device = Device::new();
    let root = install(&device, RED);
    let game = device.add_game_at(INSTALLED, &install_saves(&root)).game;
    set_icon(&device, game.id, BLUE);

    device.core.scan_installed().unwrap();
    assert_eq!(icon_color(&device, &game.uuid), Some(BLUE));
}

#[test]
fn a_remote_icon_is_not_replaced_by_the_installs() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    let b = Device::synced(remote.path());
    let root = install(&b, RED);
    let game = a.add_game_at(INSTALLED, &install_saves(&root)).game;
    set_icon(&a, game.id, GREEN);
    a.core.refresh_sync().unwrap();

    // B adopts it because it's installed there.
    b.core.refresh_sync().unwrap();
    assert_eq!(icon_color(&b, &game.uuid), Some(GREEN));
    b.core.scan_installed().unwrap();
    b.core.refresh_sync().unwrap();
    assert_eq!(icon_color(&b, &game.uuid), Some(GREEN));
}

#[test]
fn an_adopted_game_without_an_icon_gets_the_scans_and_shares_it() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    let b = Device::synced(remote.path());
    let root = install(&b, RED);
    let game = a.add_game_at(INSTALLED, &install_saves(&root)).game;
    a.core.refresh_sync().unwrap();

    // B adopts it because the sync's scan finds it installed.
    b.core.refresh_sync().unwrap();
    assert_eq!(icon_color(&b, &game.uuid), Some(RED));
    assert!(remote_icon(remote.path(), &game).is_file(), "pushed");

    a.core.refresh_sync().unwrap();
    assert_eq!(icon_color(&a, &game.uuid), Some(RED));
}

#[test]
fn the_device_that_has_it_installed_fills_the_icon_and_the_other_gets_it_next_sync() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    let b = Device::synced(remote.path());
    a.write("save.dat", "1");
    let game = a.add_game(INSTALLED).game;
    a.core.refresh_sync().unwrap();
    b.core
        .adopt_remote_game(Uuid::parse_str(&game.uuid).unwrap())
        .unwrap();
    assert_eq!(icon_color(&b, &game.uuid), None, "not installed anywhere");

    // Installed on B later. With a remote, the scan leaves the write to the
    // sync, after its pull.
    let _root = install(&b, RED);
    b.core.scan_installed().unwrap();
    assert_eq!(icon_color(&b, &game.uuid), None);
    b.core.refresh_sync().unwrap();
    assert_eq!(icon_color(&b, &game.uuid), Some(RED));

    assert_eq!(icon_color(&a, &game.uuid), None);
    a.core.refresh_sync().unwrap();
    assert_eq!(icon_color(&a, &game.uuid), Some(RED));
}

#[test]
fn a_rename_another_device_made_wins_over_the_fill() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    let b = Device::synced(remote.path());
    a.write("save.dat", "1");
    let game = a.add_game(INSTALLED).game;
    a.core.refresh_sync().unwrap();
    b.core
        .adopt_remote_game(Uuid::parse_str(&game.uuid).unwrap())
        .unwrap();
    let _root = install(&b, RED);
    b.core.scan_installed().unwrap();

    // A renames it before B's next sync.
    a.core
        .save_game(
            worldline_core::models::GameForm {
                id: Some(game.id),
                title: "Ember Trail Remastered".into(),
            },
            None,
        )
        .unwrap();
    a.core.refresh_sync().unwrap();

    b.core.refresh_sync().unwrap();
    let on_b = b
        .game_by_title("Ember Trail Remastered")
        .expect("renamed on B")
        .game;
    assert_eq!(on_b.uuid, game.uuid);
    assert_eq!(on_b.icon_path, None, "matched by its old title");
}

/// `worldline sync` opens a core that hasn't scanned installs yet; an
/// installed game without an icon still gets its install's on that sync.
#[test]
fn a_fresh_cli_sync_fills_an_installed_games_icon() {
    let remote = tempfile::tempdir().unwrap();
    let device = Device::synced(remote.path());
    let root = install(&device, RED);
    let game = device.add_game_at(INSTALLED, &install_saves(&root)).game;
    let Device { core, data, saves } = device;
    drop(core);

    let (core, _) = WorldlineCore::open(data.path(), OpenOptions::default()).unwrap();
    let device = Device { core, data, saves };
    device.core.refresh_sync().unwrap();
    assert_eq!(icon_color(&device, &game.uuid), Some(RED));
    assert!(remote_icon(remote.path(), &game).is_file(), "pushed");
}

/// An install's icon is automatic; when the install later has a sharper one,
/// it replaces the old one and reaches the other device. The same icon again
/// rewrites nothing.
#[test]
fn a_sharper_install_icon_replaces_an_automatic_one_on_every_device() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    let b = Device::synced(remote.path());
    let root = install(&b, RED);
    install_icon(&root, RED, 32);
    let uuid = a.add_game_at(INSTALLED, &install_saves(&root)).game.uuid;
    a.core.refresh_sync().unwrap();
    b.core.refresh_sync().unwrap();
    assert_eq!(icon_color(&b, &uuid), Some(RED));
    assert_eq!(icon_size(&b, &uuid), (32, 32), "not upscaled");
    assert_eq!(meta(&b, &uuid).icon_source, IconSource::Auto);
    a.core.refresh_sync().unwrap();
    assert_eq!(meta(&a, &uuid).icon_source, IconSource::Auto);

    let before = meta(&b, &uuid);
    b.core.refresh_sync().unwrap();
    assert_eq!(meta(&b, &uuid), before, "the same icon writes nothing");

    // A game update brings a sharper icon, and B's preview of the old one is
    // past its window.
    install_icon(&root, GREEN, 128);
    fs::remove_dir_all(b.core.data_dir.join("cache").join("icon_cache_v2")).unwrap();
    b.core.refresh_sync().unwrap();
    assert_eq!(icon_color(&b, &uuid), Some(GREEN));
    assert_eq!(icon_size(&b, &uuid), (128, 128));
    assert_eq!(meta(&b, &uuid).icon_source, IconSource::Auto);

    a.core.refresh_sync().unwrap();
    assert_eq!(icon_color(&a, &uuid), Some(GREEN));
    assert_eq!(meta(&a, &uuid), meta(&b, &uuid));
}

// ---- the icon's revision ------------------------------------------------------------

/// The game's icon revision on `device`, after checking it names the bytes
/// shown at its path.
fn revision(device: &Device, uuid: &str) -> Option<String> {
    let game = game(device, uuid);
    if let Some(path) = &game.icon_path {
        let shown = worldline_core::icon::sha256(&fs::read(path).unwrap());
        assert_eq!(game.icon_sha256.as_deref(), Some(shown.as_str()));
    }
    game.icon_sha256
}

#[test]
fn an_icon_replaced_at_the_same_path_gets_a_new_revision_on_every_device() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    a.write("save.dat", "1");
    let game_a = a.add_game("Revised").game;
    let (id, uuid) = (game_a.id, game_a.uuid);
    assert_eq!((game_a.icon_path, game_a.icon_sha256), (None, None));
    let sized = |color: [u8; 4], size: u32| {
        let path = a.saves.path().join(format!("{size}.png"));
        RgbaImage::from_pixel(size, size, Rgba(color))
            .save(&path)
            .unwrap();
        path.to_string_lossy().into_owned()
    };

    a.core
        .set_game_icon_as(id, &sized(RED, 32), IconSource::Auto)
        .unwrap();
    let path = game(&a, &uuid).icon_path.expect("an icon");
    let small = revision(&a, &uuid).expect("a revision");

    // A sharper automatic icon, then the user's: same path, new revision.
    a.core
        .set_game_icon_as(id, &sized(RED, 128), IconSource::Auto)
        .unwrap();
    let sharper = revision(&a, &uuid).expect("a revision");
    assert_ne!(sharper, small);
    a.core.set_game_icon(id, &sized(GREEN, 16)).unwrap();
    let picked = revision(&a, &uuid).expect("a revision");
    assert_ne!(picked, sharper);
    assert_eq!(game(&a, &uuid).icon_path.as_ref(), Some(&path));

    // Nothing replaced, nothing changed; reindexing keeps it.
    a.core
        .set_game_icon_as(id, &sized(BLUE, 256), IconSource::Auto)
        .unwrap();
    assert_eq!(revision(&a, &uuid).as_ref(), Some(&picked));
    a.core.reindex().unwrap();
    assert_eq!(revision(&a, &uuid).as_ref(), Some(&picked));

    // Another device gets the revision with the icon, and the next one the
    // same way at its own unchanged path.
    a.core.refresh_sync().unwrap();
    let b = Device::synced(remote.path());
    b.core
        .adopt_remote_game(Uuid::parse_str(&uuid).unwrap())
        .unwrap();
    let path_b = game(&b, &uuid).icon_path.expect("arrived");
    assert_eq!(revision(&b, &uuid).as_ref(), Some(&picked));
    a.core.set_game_icon(id, &sized(BLUE, 64)).unwrap();
    let repicked = revision(&a, &uuid).expect("a revision");
    assert_ne!(repicked, picked);
    a.core.refresh_sync().unwrap();
    b.core.refresh_sync().unwrap();
    assert_eq!(game(&b, &uuid).icon_path, Some(path_b));
    assert_eq!(revision(&b, &uuid), Some(repicked));
}

#[test]
fn an_icon_that_has_not_arrived_has_no_revision() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    a.write("save.dat", "1");
    let id = a.add_game("Pending").game.id;
    set_icon(&a, id, BLUE);
    a.core.refresh_sync().unwrap();
    let on_a = a.game(id).game;
    fs::remove_file(remote_icon(remote.path(), &on_a)).unwrap();

    let b = Device::synced(remote.path());
    b.core
        .adopt_remote_game(Uuid::parse_str(&on_a.uuid).unwrap())
        .unwrap();
    assert_eq!(revision(&b, &on_a.uuid), None, "not arrived");
    a.core.refresh_sync().unwrap();
    b.core.refresh_sync().unwrap();
    assert_eq!(revision(&b, &on_a.uuid), on_a.icon_sha256);
}

#[test]
fn a_database_from_before_the_icon_revision_gains_it_in_place() {
    use diesel::{Connection, RunQueryDsl, SqliteConnection};
    let device = Device::new();
    device.write("save.dat", "1");
    let id = device.add_game("Older").game.id;
    set_icon(&device, id, RED);
    let uuid = device.game(id).game.uuid;
    let Device { core, data, saves } = device;
    drop(core);
    // The index as the previous release left it.
    let db = data.path().join("database.db");
    let mut conn = SqliteConnection::establish(&db.to_string_lossy()).unwrap();
    diesel::sql_query("ALTER TABLE games DROP COLUMN icon_sha256")
        .execute(&mut conn)
        .unwrap();
    diesel::sql_query("DELETE FROM __diesel_schema_migrations WHERE version = (SELECT MAX(version) FROM __diesel_schema_migrations)")
        .execute(&mut conn)
        .unwrap();
    drop(conn);

    let (core, _) = WorldlineCore::open(data.path(), OpenOptions::default()).unwrap();
    assert!(core.moved_old_data().is_none(), "kept, not moved aside");
    let device = Device { core, data, saves };
    assert!(revision(&device, &uuid).is_some());
}

#[test]
fn an_old_icon_is_not_shown_as_a_new_one_that_has_not_arrived() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    a.write("save.dat", "1");
    let id = a.add_game("Replaced").game.id;
    set_icon(&a, id, RED);
    a.core.refresh_sync().unwrap();
    let uuid = a.game(id).game.uuid;
    let b = Device::synced(remote.path());
    b.core
        .adopt_remote_game(Uuid::parse_str(&uuid).unwrap())
        .unwrap();
    assert_eq!(icon_color(&b, &uuid), Some(RED));

    // A picks a new icon; its metadata reaches B before the icon does.
    set_icon(&a, id, GREEN);
    a.core.refresh_sync().unwrap();
    let on_a = a.game(id).game;
    fs::remove_file(remote_icon(remote.path(), &on_a)).unwrap();
    b.core.refresh_sync().unwrap();
    assert_eq!(meta(&b, &uuid).icon_sha256, on_a.icon_sha256);
    assert_eq!(revision(&b, &uuid), None, "the old icon is not the new one");

    // The new icon arrives on a later sync and is shown under its revision.
    a.core.refresh_sync().unwrap();
    b.core.refresh_sync().unwrap();
    assert_eq!(revision(&b, &uuid), on_a.icon_sha256);
    assert_eq!(icon_color(&b, &uuid), Some(GREEN));
}
