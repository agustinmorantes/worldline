use tempfile::tempdir;
use std::fs;
use std::sync::Arc;
use core::time::Duration;
use savegame_timeline_core::models::*;
use savegame_timeline_core::persistence::sqlite::{SqliteGameRepository, SqliteVersionRepository};
use savegame_timeline_core::TimelineCore;
use savegame_timeline_core::MIGRATIONS;
use diesel::r2d2::{ConnectionManager, Pool};
use diesel::SqliteConnection;
use diesel_migrations::MigrationHarness;

fn init_db() -> Pool<ConnectionManager<SqliteConnection>> {
    let manager = ConnectionManager::<SqliteConnection>::new(":memory:");
    let pool = Pool::builder().max_size(1).build(manager).unwrap();
    let mut conn = pool.get().unwrap();
    conn.run_pending_migrations(MIGRATIONS).unwrap();
    pool
}

#[test]
fn test_full_e2e_flow() {
    let pool = init_db();
    let games_repo = Arc::new(SqliteGameRepository::new(pool.clone()));
    let versions_repo = Arc::new(SqliteVersionRepository::new(pool.clone()));
    
    let working_base = tempdir().unwrap();
    let source_dir = tempdir().unwrap();
    
    let save_file = source_dir.path().join("save.dat");
    fs::write(&save_file, "initial text").unwrap();
    
    let icons_dir = tempdir().unwrap();
    let (core, notif_rx) = TimelineCore::new(
        games_repo.clone(),
        versions_repo.clone(),
        working_base.path().to_path_buf(),
        icons_dir.path().to_path_buf(),
        Some(0.1)
    ).unwrap();
    
    let form = GameForm { id: None, title: "E2E Test", active_version_id: None, icon_path: None };
    let path_str = source_dir.path().to_str().unwrap();
    let game = core.save_game(form, Some(vec![path_str])).unwrap();
    
    std::thread::sleep(Duration::from_millis(50));
    
    fs::write(&save_file, "modified text").unwrap();
    
    std::thread::sleep(Duration::from_millis(2500));
    
    let mut version_created = false;
    let mut change_detected = false;
    
    while let Ok(msg) = notif_rx.try_recv() {
        match msg {
            savegame_timeline_core::detection::Notification::VersionCreated { game_id, version_number } => {
                assert_eq!(game_id, game.id);
                assert_eq!(version_number, 1);
                version_created = true;
            }
            savegame_timeline_core::detection::Notification::ChangeDetected { game_id } => {
                assert_eq!(game_id, game.id);
                change_detected = true;
            }
            _ => {}
        }
    }
    
    assert!(version_created, "Version was not created automatically");
    assert!(change_detected, "Change was not detected automatically");
    
    let full_game = core.get_game(game.id).unwrap();
    let version_folder = working_base.path()
        .join(game.id.to_string())
        .join(full_game.versions.last().unwrap().version_number.to_string())
        .join(full_game.tracked_dirs[0].id.to_string());
        
    let copied_file = version_folder.join("save.dat");
    assert!(copied_file.exists());
    let content = fs::read_to_string(copied_file).unwrap();
    assert_eq!(content, "modified text");
    
    // Tag version test via Facade
    let version_id = full_game.versions.last().unwrap().id;
    let tagged = core.tag_version(version_id, "test_tag".to_string()).unwrap();
    assert_eq!(tagged.tag.unwrap(), "test_tag");
    
    // Restore version test via Facade
    // Overwrite the original file with something else
    fs::write(&save_file, "corrupted game!").unwrap();
    
    // Restore the correct version
    core.restore_version(version_id).unwrap();
    
    // Should be restored
    let restored_content = fs::read_to_string(&save_file).unwrap();
    assert_eq!(restored_content, "modified text");
}

#[test]
fn test_negative_facade_paths() {
    let pool = init_db();
    let games_repo = Arc::new(SqliteGameRepository::new(pool.clone()));
    let versions_repo = Arc::new(SqliteVersionRepository::new(pool.clone()));
    
    let working_base = tempdir().unwrap();
    let icons_dir = tempdir().unwrap();
    
    let (core, _notif_rx) = TimelineCore::new(
        games_repo,
        versions_repo,
        working_base.path().to_path_buf(),
        icons_dir.path().to_path_buf(),
        Some(0.0)
    ).unwrap();
    
    // restore bad version
    let res = core.restore_version(9999);
    assert!(res.is_err(), "Expected error when restoring non-existent version");
}
