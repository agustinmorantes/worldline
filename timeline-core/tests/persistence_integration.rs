use savegame_timeline_core::models::*;
use savegame_timeline_core::persistence::sqlite::{SqliteGameRepository, SqliteVersionRepository};
use savegame_timeline_core::persistence::{GameRepository, VersionRepository};
use savegame_timeline_core::MIGRATIONS;
use diesel::r2d2::{ConnectionManager, Pool};
use diesel::SqliteConnection;
use diesel_migrations::MigrationHarness;

fn init_db() -> Pool<ConnectionManager<SqliteConnection>> {
    let manager = ConnectionManager::<SqliteConnection>::new(":memory:");
    // Pin to 1 connection to avoid multiple memory databases
    let pool = Pool::builder().max_size(1).build(manager).unwrap();
    let mut conn = pool.get().unwrap();
    conn.run_pending_migrations(MIGRATIONS).unwrap();
    pool
}

#[test]
fn test_game_crud() {
    let pool = init_db();
    let games_repo = SqliteGameRepository::new(pool.clone());
    
    // Create
    let form = GameForm {
        id: None,
        title: "Integration Test Game",
        active_version_id: None,
        icon_path: None,
    };
    
    let game = games_repo.save_game(form, Some(vec!["/test/path"])).unwrap();
    assert_eq!(game.title, "Integration Test Game");
    
    // Read
    let full_game = games_repo.get_game(game.id).unwrap();
    assert_eq!(full_game.tracked_dirs.len(), 1);
    assert_eq!(full_game.tracked_dirs[0].path, "/test/path");
    
    // Update
    let update_form = GameForm {
        id: Some(game.id),
        title: "Updated Game",
        active_version_id: None,
        icon_path: None,
    };
    let updated = games_repo.save_game(update_form, Some(vec!["/new/path"])).unwrap();
    assert_eq!(updated.title, "Updated Game");
    
    let new_full = games_repo.get_game(game.id).unwrap();
    assert_eq!(new_full.tracked_dirs.len(), 1);
    assert_eq!(new_full.tracked_dirs[0].path, "/new/path");

    // test get_games
    let all_games = games_repo.get_games().unwrap();
    assert_eq!(all_games.len(), 1);
    assert_eq!(all_games[0].game.title, "Updated Game");
    
    // Delete
    games_repo.delete_game(game.id).unwrap();
    assert!(games_repo.get_game(game.id).is_err());
    
    let all_games_after = games_repo.get_games().unwrap();
    assert_eq!(all_games_after.len(), 0);
}

#[test]
fn test_version_operations() {
    let pool = init_db();
    let games_repo = SqliteGameRepository::new(pool.clone());
    let versions_repo = SqliteVersionRepository::new(pool.clone());
    
    // Setup game
    let form = GameForm { id: None, title: "Game", active_version_id: None, icon_path: None };
    let game = games_repo.save_game(form, None).unwrap();
    
    // Create version
    let version = versions_repo.create_version(game.id).unwrap();
    assert_eq!(version.version.version_number, 1);
    
    // Check if game was updated with activeversion
    let g = games_repo.get_game(game.id).unwrap();
    assert_eq!(g.game.active_version_id, Some(version.version.id));
    
    // Tag version
    let tagged = versions_repo.tag_version(version.version.id, "my_tag".to_string()).unwrap();
    assert_eq!(tagged.tag.unwrap(), "my_tag");
}

#[test]
fn test_negative_paths() {
    let pool = init_db();
    let games_repo = SqliteGameRepository::new(pool.clone());
    
    // get non-existent game
    let res = games_repo.get_game(9999);
    assert!(res.is_err(), "Expected error when fetching non-existent game");
    
    // delete non-existent game
    // diesel delete with 0 affected rows doesn't error out, it returns Ok(0)
    let delete_res = games_repo.delete_game(9999);
    assert!(delete_res.is_ok(), "Deleting a non-existent game should safely return Ok");
}
