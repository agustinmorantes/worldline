use crate::detection::CommandSender;
use crate::models::*;
use crate::{detection, DbPool};
use anyhow::{Context, Result};
use diesel::prelude::*;
use tauri::{generate_handler, State};

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
fn get_games(db: State<DbPool>) -> Result<Vec<FullGame>, String> {
    use crate::schema::games::dsl::*;

    let conn = &mut db
        .get()
        .context("Error getting db connection")
        .map_err(|e| e.to_string())?;

    let all_games = games
        .select(Game::as_select())
        .load(conn)
        .context("Error loading games")
        .map_err(|e| e.to_string())?;

    let dirs = TrackedDir::belonging_to(&all_games)
        .select(TrackedDir::as_select())
        .load(conn)
        .map_err(|e| e.to_string())?;

    let res = dirs
        .grouped_by(&all_games)
        .into_iter()
        .zip(all_games)
        .map(|(dirs, game)| FullGame {
            game,
            tracked_dirs: dirs,
        })
        .collect();

    Ok(res)
}

#[tauri::command]
fn get_game(id: i32, db: State<DbPool>) -> Result<FullGame, String> {
    use crate::schema::games::dsl;

    let conn = &mut db
        .get()
        .context("Error getting db connection")
        .map_err(|e| e.to_string())?;

    let game: Game = dsl::games
        .filter(dsl::id.eq(id))
        .select(Game::as_select())
        .first(conn)
        .context("Error loading game")
        .map_err(|e| e.to_string())?;

    let dirs: Vec<TrackedDir> = TrackedDir::belonging_to(&game)
        .select(TrackedDir::as_select())
        .load(conn)
        .map_err(|e| e.to_string())?;

    Ok(FullGame {
        game,
        tracked_dirs: dirs,
    })
}

#[tauri::command]
fn save_game(
    game: GameForm,
    dirs: Option<Vec<&str>>,
    db: State<DbPool>,
    cmd_tx: State<CommandSender>,
) -> Result<Game, String> {
    use crate::schema::games;
    use crate::schema::tracked_dirs;
    use crate::schema::tracked_dirs::dsl;

    let conn = &mut db
        .get()
        .context("Error getting db connection")
        .map_err(|e| e.to_string())?;

    let res = diesel::insert_into(games::table)
        .values(&game)
        .on_conflict(games::id)
        .do_update()
        .set(&game)
        .returning(Game::as_returning())
        .get_result(conn)
        .context("Error saving new game")
        .map_err(|e| e.to_string())?;

    if let Some(dirs) = dirs {
        diesel::delete(
            dsl::tracked_dirs.filter(
                tracked_dirs::game_id
                    .eq(res.id)
                    .and(tracked_dirs::path.ne_all(&dirs)),
            ),
        )
        .execute(conn)
        .context("Error deleting old tracked dirs")
        .map_err(|e| e.to_string())?;

        let new_dirs: Vec<TrackedDirForm> = dirs
            .iter()
            .map(|path| TrackedDirForm {
                game_id: res.id,
                path,
                id: None,
            })
            .collect();

        // Can't do bulk insert with on_conflict do nothing.
        for dir in &new_dirs {
            diesel::insert_into(tracked_dirs::table)
                .values(dir)
                .on_conflict(tracked_dirs::path)
                .do_nothing()
                .execute(conn)
                .context("Error saving tracked dirs")
                .map_err(|e| e.to_string())?;
        }

        // Notify detection system of updated dirs
        let res = detection::reregister_dirs(&cmd_tx, &db)
            .map_err(|e| e.to_string());
        if let Err(err) = res {
            return Err(err.to_string());
        }
    }

    Ok(res)
}

#[tauri::command]
fn delete_game(id: i32, db: State<DbPool>) -> Result<(), String> {
    use crate::schema::games::dsl;

    let conn = &mut db
        .get()
        .context("Error getting db connection")
        .map_err(|e| e.to_string())?;

    diesel::delete(dsl::games.filter(dsl::id.eq(id)))
        .execute(conn)
        .context("Error deleting game")
        .map_err(|e| e.to_string())?;

    Ok(())
}

pub fn add_handlers(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder.invoke_handler(generate_handler![
        greet,
        get_games,
        get_game,
        save_game,
        delete_game
    ])
}
