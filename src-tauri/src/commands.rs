use tauri::{generate_handler, State};
use diesel::prelude::*;
use anyhow::{Context, Result};
use crate::{DbPool};
use crate::models::*;
use crate::schema::games;
use crate::schema::games::dsl::*;

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {

    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
fn get_games(db: State<DbPool>) -> Result<Vec<Game>, String> {
    let conn = &mut db.get()
        .context("Error getting db connection")
        .map_err(|e| e.to_string())?;

    let res = games
        .select(Game::as_select())
        .load(conn)
        .context("Error loading games")
        .map_err(|e| e.to_string())?;

    Ok(res)
}

#[tauri::command]
fn create_game(game: NewGame, db: State<DbPool>) -> Result<Game, String> {
    let conn = &mut db.get()
        .context("Error getting db connection")
        .map_err(|e| e.to_string())?;

    let res = diesel::insert_into(games::table)
        .values(&game)
        .returning(Game::as_returning())
        .get_result(conn)
        .context("Error saving new game")
        .map_err(|e| e.to_string())?;

    Ok(res)
}

pub fn add_handlers(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder
        .invoke_handler(generate_handler![
            greet,
            get_games,
            create_game
        ])
}
