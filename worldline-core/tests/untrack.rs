//! Untracking (ADR 0006): a device stops tracking a game while its history
//! stays on the remote, and can adopt it again.

mod common;

use common::Device;
use std::fs;
use uuid::Uuid;
use worldline_core::store::Store;
use worldline_core::sync::Divergence;

fn remote_versions(remote: &std::path::Path, game: Uuid) -> usize {
    Store::open_existing(remote)
        .unwrap()
        .game(game)
        .unwrap()
        .unwrap()
        .versions()
        .count()
}

/// Makes the install scanner report `title` as installed on `device`.
fn install(device: &Device, title: &str) -> tempfile::TempDir {
    let cache = device.data.path().join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(
        cache.join("manifest.yaml"),
        format!("{title}:\n  files:\n    <base>/saves:\n      tags: [save]\n"),
    )
    .unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join(title).join("saves")).unwrap();
    fs::write(root.path().join(title).join("saves").join("x"), "x").unwrap();
    device
        .core
        .add_scan_root(&root.path().to_string_lossy())
        .unwrap();
    root
}

#[test]
fn untracking_leaves_the_history_on_the_remote_and_adopting_brings_it_back() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    a.write("save.dat", "1");
    let game = a.add_game("Wanderer");
    let uuid = Uuid::parse_str(&game.game.uuid).unwrap();
    a.write("save.dat", "2");
    a.checkpoint(game.game.id);
    a.core.refresh_sync().unwrap();
    let b = Device::synced(remote.path());
    let on_b = b.core.adopt_remote_game(uuid).unwrap();

    // Uncaptured progress is recorded and published before the game goes.
    a.write("save.dat", "3");
    a.core.untrack_game(game.game.id).unwrap();
    assert!(a.core.get_games().unwrap().is_empty());
    assert_eq!(remote_versions(remote.path(), uuid), 3);

    // B keeps the game and A's last version, though A no longer points at it.
    let report = b.core.refresh_sync().unwrap();
    let status = report.games.iter().find(|s| s.game_uuid == uuid).unwrap();
    assert_eq!(status.relation, Divergence::LocalOnly);
    let versions = b.game(on_b.game.id).versions;
    assert_eq!(versions.len(), 3);
    let latest = versions.iter().find(|v| v.version_number == 3).unwrap();
    a.write("save.dat", "2"); // B's machine still holds version 2
    b.core.restore_version(latest.id).unwrap();
    b.core.push_sync().unwrap();

    // A lists it among the remote games and, though installed, doesn't adopt it.
    let _root = install(&a, "Wanderer");
    let report = a.core.refresh_sync().unwrap();
    let listed = report.remote_games.iter().find(|g| g.uuid == uuid).unwrap();
    assert!(listed.installed && !listed.adopted, "{listed:?}");
    assert!(a.core.get_games().unwrap().is_empty());
    a.core.reindex().unwrap();
    assert!(
        a.core.get_games().unwrap().is_empty(),
        "reindex keeps it out"
    );

    // Adopting it again restores its history.
    let back = a.core.adopt_remote_game(uuid).unwrap();
    assert_eq!(back.versions.len(), 3);
    assert_eq!(a.read("save.dat").as_deref(), Some("3"));
    let report = a.core.refresh_sync().unwrap();
    assert!(
        report
            .remote_games
            .iter()
            .any(|g| g.uuid == uuid && g.adopted)
    );
    a.core.reindex().unwrap();
    assert_eq!(a.core.get_games().unwrap().len(), 1);
}

#[test]
fn importing_an_untracked_games_history_tracks_it_again() {
    let remote = tempfile::tempdir().unwrap();
    let device = Device::synced(remote.path());
    device.write("save.dat", "1");
    let game = device.add_game("Bundled");
    device.core.push_sync().unwrap();
    let out = tempfile::tempdir().unwrap();
    let bundle = out.path().join("bundle");
    device
        .core
        .export_game_history(game.game.id, &bundle)
        .unwrap();
    device.core.untrack_game(game.game.id).unwrap();

    let summary = worldline_core::WorldlineCore::inspect_history_bundle(&bundle).unwrap();
    let map = summary
        .active_tracked_dirs
        .iter()
        .map(|dir| (dir.uuid, device.save_dir()))
        .collect();
    device.core.import_game_history(&bundle, &map).unwrap();
    assert_eq!(device.core.get_games().unwrap().len(), 1);
}

#[test]
fn a_game_that_is_not_on_the_remote_cannot_be_untracked() {
    let device = Device::new();
    device.write("save.dat", "1");
    let game = device.add_game("Local only");
    let error = device.core.untrack_game(game.game.id).unwrap_err();
    assert!(format!("{error:#}").contains("sync"), "{error:#}");
    assert_eq!(device.core.get_games().unwrap().len(), 1);

    let remote = tempfile::tempdir().unwrap();
    let device = Device::synced(remote.path());
    device.write("save.dat", "1");
    let game = device.add_game("Not pushed");
    let error = device.core.untrack_game(game.game.id).unwrap_err();
    assert!(format!("{error:#}").contains("sync folder"), "{error:#}");
    assert_eq!(device.core.get_games().unwrap().len(), 1);
}
