mod common;

use common::Device;
use std::collections::HashMap;
use std::fs;
use uuid::Uuid;

fn exported(device: &Device) -> (tempfile::TempDir, std::path::PathBuf, i32) {
    device.write("save.dat", "one");
    let game = device.add_game("Exported");
    device.write("save.dat", "two");
    let tip = device.checkpoint(game.game.id);
    device.core.tag_version(tip.id, "boss".into()).unwrap();
    let out = tempfile::tempdir().unwrap();
    let path = out.path().join("bundle");
    let report = device
        .core
        .export_game_history(game.game.id, &path)
        .unwrap();
    assert_eq!(report.version_count, 2);
    assert!(
        device
            .core
            .export_game_history(game.game.id, &path)
            .is_err(),
        "never overwrites"
    );
    (out, path, game.game.id)
}

fn mapping(
    bundle: &std::path::Path,
    target: &std::path::Path,
) -> HashMap<Uuid, std::path::PathBuf> {
    let summary = savegame_timeline_core::TimelineCore::inspect_history_bundle(bundle).unwrap();
    summary
        .active_tracked_dirs
        .iter()
        .map(|dir| (dir.uuid, target.to_path_buf()))
        .collect()
}

#[test]
fn round_trip_imports_history_without_restoring_and_reimport_is_idempotent() {
    let a = Device::new();
    let (_out, bundle, _) = exported(&a);
    fs::write(bundle.join("desktop.ini"), b"[.ShellClassInfo]").unwrap();

    let b = Device::new();
    fs::create_dir_all(b.save_dir()).unwrap();
    b.write("save.dat", "local progress");
    assert!(
        b.core
            .import_game_history(&bundle, &HashMap::new())
            .is_err(),
        "paths must be mapped"
    );
    let map = mapping(&bundle, &b.save_dir());
    let report = b.core.import_game_history(&bundle, &map).unwrap();
    assert_eq!(report.version_count, 2);
    assert_eq!(
        b.read("save.dat").as_deref(),
        Some("local progress"),
        "import never restores"
    );

    let game = b.game_by_title("Exported").unwrap();
    assert_eq!(game.game.icon_path, None);
    assert_eq!(game.tracked_dirs[0].path, b.save_dir().to_string_lossy());
    let versions = b
        .core
        .get_slot(game.active_slot.as_ref().unwrap().id)
        .unwrap()
        .versions;
    assert_eq!(versions.len(), 2);
    assert!(versions.iter().any(|v| v.tag.as_deref() == Some("boss")));

    b.core.import_game_history(&bundle, &map).unwrap();
    assert_eq!(b.core.get_games().unwrap().len(), 1);
    assert_eq!(
        b.core
            .get_slot(game.active_slot.unwrap().id)
            .unwrap()
            .versions
            .len(),
        2
    );
    let oldest = versions.iter().min_by_key(|v| v.version_number).unwrap();
    b.core.restore_version(oldest.id).unwrap();
    assert_eq!(b.read("save.dat").as_deref(), Some("one"));
}

#[test]
fn a_damaged_bundle_is_rejected_before_anything_is_written() {
    let a = Device::new();
    let (_out, bundle, _) = exported(&a);
    for shard in fs::read_dir(bundle.join("chunks")).unwrap() {
        for chunk in fs::read_dir(shard.unwrap().path()).unwrap() {
            fs::write(chunk.unwrap().path(), b"garbage").unwrap();
        }
    }
    let b = Device::new();
    let map = mapping(&bundle, &b.save_dir());
    assert!(b.core.import_game_history(&bundle, &map).is_err());
    assert!(b.core.get_games().unwrap().is_empty());
    assert!(
        fs::read_dir(b.core.repository_root().join("games"))
            .map_or(true, |mut d| d.next().is_none())
    );
}

#[test]
fn title_and_path_collisions_are_refused_before_writing() {
    let a = Device::new();
    let (_out, bundle, _) = exported(&a);
    let b = Device::new();
    b.write("save.dat", "mine");
    b.add_game("exported");
    let other = tempfile::tempdir().unwrap();
    assert!(
        b.core
            .import_game_history(&bundle, &mapping(&bundle, other.path()))
            .is_err()
    );

    let c = Device::new();
    c.write("save.dat", "mine");
    c.add_game("Something Else");
    assert!(
        c.core
            .import_game_history(&bundle, &mapping(&bundle, &c.save_dir()))
            .is_err()
    );
    assert_eq!(c.core.get_games().unwrap().len(), 1);
}

#[test]
fn a_bundle_with_cyclic_ancestry_is_rejected_by_inspect_and_import() {
    let a = Device::new();
    let (_out, bundle, _) = exported(&a);
    let b = Device::new();
    let map = mapping(&bundle, &b.save_dir());
    // Make the root's parent its own child.
    let mut metas = Vec::new();
    let games = bundle.join("games");
    let slots = games.join(only_child(&games)).join("slots");
    let versions = slots.join(only_child(&slots)).join("versions");
    for dir in fs::read_dir(&versions).unwrap() {
        let path = dir.unwrap().path().join("metadata.json");
        let json: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        metas.push((path, json));
    }
    let uuid = |json: &serde_json::Value| json["uuid"].clone();
    let child = metas
        .iter()
        .find(|(_, json)| !json["parent_uuid"].is_null())
        .map(|(_, json)| uuid(json))
        .unwrap();
    let (root_path, root) = metas
        .iter_mut()
        .find(|(_, json)| json["parent_uuid"].is_null())
        .unwrap();
    root["parent_uuid"] = child;
    fs::write(root_path, serde_json::to_vec(root).unwrap()).unwrap();

    assert!(savegame_timeline_core::TimelineCore::inspect_history_bundle(&bundle).is_err());
    assert!(b.core.import_game_history(&bundle, &map).is_err());
    assert!(b.core.get_games().unwrap().is_empty());
    assert!(
        fs::read_dir(b.core.repository_root().join("games"))
            .map_or(true, |mut d| d.next().is_none())
    );
}

fn only_child(dir: &std::path::Path) -> std::ffi::OsString {
    let mut names = fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name());
    let name = names.next().unwrap();
    assert!(names.next().is_none());
    name
}
