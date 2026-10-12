mod common;

use chrono::{DateTime, Utc};
use common::Device;
use std::collections::HashMap;
use std::fs::{self, File, FileTimes};
use worldline_core::models::GameForm;

fn dated(device: &Device, name: &str, content: &str, iso: &str) -> DateTime<Utc> {
    device.write(name, content);
    let at: DateTime<Utc> = iso.parse().unwrap();
    File::options()
        .write(true)
        .open(device.save_dir().join(name))
        .unwrap()
        .set_times(FileTimes::new().set_modified(at.into()))
        .unwrap();
    at
}

#[test]
fn importing_existing_saves_uses_latest_nested_file_and_keeps_capture_date() {
    let d = Device::new();
    dated(&d, "old.dat", "old", "2020-01-01T00:00:00Z");
    let played = dated(&d, "nested/save.dat", "latest", "2021-02-03T04:05:06Z");
    let game = d.add_game("Old game");
    assert_eq!(game.last_played_at, Some(played.naive_utc()));
    assert!(game.versions[0].created_at > played.naive_utc());
    d.core.reindex().unwrap();
    assert_eq!(d.game(game.game.id).last_played_at, game.last_played_at);
    assert_eq!(
        d.core.get_games().unwrap()[0].last_played_at,
        game.last_played_at
    );

    let version = d.checkpoint(game.game.id);
    assert_eq!(version.id, game.versions[0].id);
    assert_eq!(d.game(game.game.id).last_played_at, game.last_played_at);
    d.write("nested/save.dat", "new progress");
    let next = d.checkpoint(game.game.id);
    assert_eq!(d.game(game.game.id).last_played_at, Some(next.created_at));
    d.core.restore_version(version.id).unwrap();
    assert_eq!(d.game(game.game.id).last_played_at, Some(next.created_at));
    let other = d
        .core
        .create_slot(game.game.id, "Other playthrough")
        .unwrap();
    assert_eq!(other.last_played_at, Some(next.created_at));
    d.core
        .switch_slot(game.game.id, game.active_slot.unwrap().id)
        .unwrap();
    assert_eq!(d.game(game.game.id).last_played_at, Some(next.created_at));
}

#[test]
fn initial_activity_ignores_excluded_files_and_future_timestamps() {
    let d = Device::new();
    fs::create_dir_all(d.save_dir()).unwrap();
    let game = d.add_game("Excluded");
    let meta_path = d
        .data
        .path()
        .join("repository")
        .join("games")
        .join(&game.game.folder_slug)
        .join("tracked-dirs")
        .join(&game.tracked_dirs[0].folder_slug)
        .join("metadata.json");
    let mut meta: serde_json::Value =
        serde_json::from_slice(&fs::read(&meta_path).unwrap()).unwrap();
    meta["exclusions"] = serde_json::json!(["*.cfg"]);
    fs::write(&meta_path, serde_json::to_vec(&meta).unwrap()).unwrap();
    d.core.reindex().unwrap();
    let played = dated(&d, "nested/save.dat", "save", "2020-01-01T00:00:00Z");
    dated(&d, "settings.cfg", "excluded", "2025-01-01T00:00:00Z");
    let game = d
        .core
        .save_game(
            GameForm {
                id: Some(game.game.id),
                title: game.game.title,
            },
            None,
        )
        .unwrap();
    // Updating a game does not capture; creating its first slot captures the included saves.
    let game = d.core.create_slot(game.id, "First saves").unwrap();
    assert_eq!(game.last_played_at, Some(played.naive_utc()));

    let future = Device::new();
    dated(&future, "save.dat", "save", "2099-01-01T00:00:00Z");
    let game = future.add_game("Future clock");
    assert!(game.last_played_at.unwrap() <= game.versions[0].created_at);
}

#[test]
fn bundles_and_legacy_metadata_keep_activity_separate_from_import_time() {
    let a = Device::new();
    let played = dated(&a, "save.dat", "save", "2020-01-01T00:00:00Z");
    let game = a.add_game("Imported history");
    let out = tempfile::tempdir().unwrap();
    let bundle = out.path().join("history.zip");
    a.core.export_game_history(game.game.id, &bundle).unwrap();
    let summary = worldline_core::WorldlineCore::inspect_history_bundle(&bundle).unwrap();
    let b = Device::new();
    fs::create_dir_all(b.save_dir()).unwrap();
    let paths = summary
        .active_tracked_dirs
        .iter()
        .map(|dir| (dir.uuid, b.save_dir()))
        .collect::<HashMap<_, _>>();
    b.core.import_game_history(&bundle, &paths).unwrap();
    assert_eq!(
        b.game_by_title("Imported history").unwrap().last_played_at,
        Some(played.naive_utc())
    );

    let version = &game.versions[0];
    let meta_path = a
        .data
        .path()
        .join("repository")
        .join("games")
        .join(&game.game.folder_slug)
        .join("slots")
        .join(&game.active_slot.unwrap().folder_slug)
        .join("versions")
        .join(&version.folder_slug)
        .join("metadata.json");
    let mut meta: serde_json::Value =
        serde_json::from_slice(&fs::read(&meta_path).unwrap()).unwrap();
    meta.as_object_mut().unwrap().remove("last_played_at");
    fs::write(&meta_path, serde_json::to_vec(&meta).unwrap()).unwrap();
    a.core.reindex().unwrap();
    assert_eq!(a.game(game.game.id).last_played_at, None);
    a.write("save.dat", "played again");
    let next = a.checkpoint(game.game.id);
    assert_eq!(a.game(game.game.id).last_played_at, Some(next.created_at));
}

#[test]
fn existing_indexes_gain_activity_without_moving_data_aside() {
    use diesel::{Connection, RunQueryDsl, SqliteConnection};
    use worldline_core::{OpenOptions, WorldlineCore};
    let d = Device::new();
    let played = dated(&d, "save.dat", "save", "2020-01-01T00:00:00Z");
    let id = d.add_game("Existing").game.id;
    let Device { core, data, saves } = d;
    drop(core);
    let mut conn =
        SqliteConnection::establish(&data.path().join("database.db").to_string_lossy()).unwrap();
    diesel::sql_query("ALTER TABLE versions DROP COLUMN last_played_at")
        .execute(&mut conn)
        .unwrap();
    diesel::sql_query(
        "DELETE FROM __diesel_schema_migrations WHERE version = '202610120000000000'",
    )
    .execute(&mut conn)
    .unwrap();
    drop(conn);
    let (core, _) = WorldlineCore::open(data.path(), OpenOptions::default()).unwrap();
    assert!(core.moved_old_data().is_none());
    let d = Device { core, data, saves };
    assert_eq!(d.game(id).last_played_at, Some(played.naive_utc()));
}
