//! Version screenshots are sidecar files: they sync (unless turned off),
//! travel in bundles, and leave with retired versions.

mod common;

use common::Device;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use worldline_core::maintenance::RetentionSettings;
use worldline_core::models::{MonitoringSettings, Version};
use worldline_core::store::Store;

/// Stores a screenshot as the watcher does once a shot meets its version.
fn attach(device: &Device, version_id: i32, jpeg: &[u8]) {
    let version = device.core.get_version(version_id).unwrap();
    Store::open_existing(device.core.repository_root())
        .unwrap()
        .write_screenshot(
            &version.game.folder_slug,
            &version.slot.folder_slug,
            &version.version.folder_slug,
            jpeg,
        )
        .unwrap();
}

fn screenshot(device: &Device, version_id: i32) -> Option<Vec<u8>> {
    device
        .core
        .screenshot_path(version_id)
        .unwrap()
        .map(|path| fs::read(path).unwrap())
}

/// Where `version` keeps its screenshot in the store at `root`.
fn screenshot_in(root: &Path, device: &Device, version_id: i32) -> PathBuf {
    let version = device.core.get_version(version_id).unwrap();
    Store::open_existing(root).unwrap().screenshot_path(
        &version.game.folder_slug,
        &version.slot.folder_slug,
        &version.version.folder_slug,
    )
}

fn sync_screenshots(device: &Device, on: bool) {
    let settings = device.core.get_monitoring_settings().unwrap();
    device
        .core
        .update_monitoring_settings(MonitoringSettings {
            sync_screenshots: on,
            ..settings
        })
        .unwrap();
}

/// The version with `uuid` in the device's copy of the game titled `title`.
fn version_on(device: &Device, title: &str, uuid: &str) -> Version {
    device
        .game_by_title(title)
        .expect("game is indexed")
        .versions
        .into_iter()
        .find(|version| version.uuid == uuid)
        .expect("version is indexed")
}

#[test]
fn settings_saved_before_screenshots_load_with_them_off_and_synced() {
    let a = Device::new();
    fs::write(
        a.data.path().join("monitoring_settings.json"),
        r#"{ "version_cooldown_minutes": 7, "snapshot_debounce_seconds": 2.0 }"#,
    )
    .unwrap();
    let settings = a.core.get_monitoring_settings().unwrap();
    assert_eq!(settings.version_cooldown_minutes, 7);
    assert!(!settings.screenshots_enabled);
    assert!(settings.sync_screenshots);
}

#[test]
fn a_screenshot_reaches_the_other_device_even_when_taken_after_the_push() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    a.write("save.dat", "1");
    let game = a.add_game("Pictured");
    let first = game.last_version.unwrap();
    a.write("save.dat", "2");
    let second = a.checkpoint(game.game.id);
    attach(&a, second.id, b"second");
    assert_eq!(screenshot(&a, first.id), None);
    assert_eq!(screenshot(&a, second.id).as_deref(), Some(&b"second"[..]));
    a.core.refresh_sync().unwrap();

    let b = Device::synced(remote.path());
    b.core
        .adopt_remote_game(uuid::Uuid::parse_str(&game.game.uuid).unwrap())
        .unwrap();
    let on_b = version_on(&b, "Pictured", &second.uuid);
    assert_eq!(screenshot(&b, on_b.id).as_deref(), Some(&b"second"[..]));

    // Taken after its version was published everywhere: healed on a later run.
    attach(&a, first.id, b"first");
    a.core.refresh_sync().unwrap();
    b.core.refresh_sync().unwrap();
    let on_b = version_on(&b, "Pictured", &first.uuid);
    assert_eq!(screenshot(&b, on_b.id).as_deref(), Some(&b"first"[..]));

    // The extra file is neither a problem nor counted as save storage.
    assert!(a.core.check_integrity(true).unwrap().is_ok());
    assert!(b.core.check_integrity(true).unwrap().is_ok());
    b.core.reindex().unwrap();
    assert!(b.core.check_integrity(true).unwrap().is_ok());
}

#[test]
fn with_sync_off_screenshots_stay_on_this_device_until_it_is_back_on() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    sync_screenshots(&a, false);
    a.write("save.dat", "1");
    let version = a.add_game("Private").last_version.unwrap();
    attach(&a, version.id, b"mine");
    a.core.refresh_sync().unwrap();
    let in_remote = screenshot_in(remote.path(), &a, version.id);
    assert!(in_remote.parent().unwrap().is_dir(), "the version synced");
    assert!(!in_remote.exists());

    sync_screenshots(&a, true);
    a.core.refresh_sync().unwrap();
    assert_eq!(fs::read(&in_remote).unwrap(), b"mine");
}

#[test]
fn a_retired_version_loses_its_screenshot_here_and_in_the_sync_folder() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    a.write("save.dat", "v1");
    let game = a.add_game("Pruned");
    for n in 2..=5 {
        a.write("save.dat", &format!("v{n}"));
        a.checkpoint(game.game.id);
    }
    let versions = a.game(game.game.id).versions;
    for version in &versions {
        attach(&a, version.id, version.uuid.as_bytes());
    }
    a.core.refresh_sync().unwrap();
    a.core
        .update_pruning_settings(RetentionSettings {
            keep_all_hours: 0,
            hourly_until_hours: 0,
            daily_until_days: 0,
            weekly_until_days: 0,
            branch_expiry_days: None,
            ..RetentionSettings::default()
        })
        .unwrap();
    let report = a
        .core
        .run_pruning_at(chrono::Utc::now() + chrono::Duration::hours(2))
        .unwrap();
    assert!(!report.retired_versions.is_empty());

    let local = a.core.repository_root().to_path_buf();
    for version in a.game(game.game.id).versions {
        let kept = !version.retired;
        assert_eq!(screenshot(&a, version.id).is_some(), kept);
        assert_eq!(screenshot_in(&local, &a, version.id).exists(), kept);
        assert_eq!(screenshot_in(remote.path(), &a, version.id).exists(), kept);
    }
    // The record stays.
    assert_eq!(a.game(game.game.id).versions.len(), versions.len());
}

#[test]
fn bundles_carry_screenshots_even_with_sync_off() {
    let a = Device::new();
    sync_screenshots(&a, false);
    a.write("save.dat", "1");
    let game = a.add_game("Exported");
    let version = game.last_version.unwrap();
    attach(&a, version.id, b"exported");
    let out = tempfile::tempdir().unwrap();
    let bundle = out.path().join("bundle");
    a.core.export_game_history(game.game.id, &bundle).unwrap();

    let b = Device::new();
    sync_screenshots(&b, false);
    let summary = worldline_core::WorldlineCore::inspect_history_bundle(&bundle).unwrap();
    let mappings: HashMap<_, _> = summary
        .active_tracked_dirs
        .iter()
        .map(|dir| (dir.uuid, b.save_dir()))
        .collect();
    b.core.import_game_history(&bundle, &mappings).unwrap();
    let imported = version_on(&b, "Exported", &version.uuid);
    assert_eq!(
        screenshot(&b, imported.id).as_deref(),
        Some(&b"exported"[..])
    );
}
