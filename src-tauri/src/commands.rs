use crate::detection::CommandSender;
use crate::models::*;
use crate::{detection, persistence, DbPool, Globals};
use anyhow::Result;
use tauri::{generate_handler, State};

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/

#[tauri::command]
fn get_games(db: State<DbPool>) -> Result<Vec<FullGame>, String> {
    persistence::games::get_games(db.inner().clone())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_game(id: i32, db: State<DbPool>) -> Result<FullGame, String> {
    persistence::games::get_game(id, db.inner().clone())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn save_game(
    game: GameForm,
    dirs: Option<Vec<&str>>,
    db: State<DbPool>,
    cmd_tx: State<CommandSender>,
) -> Result<Game, String> {
    persistence::games::save_game(
        game,
        dirs,
        db.inner().clone(),
        cmd_tx.inner().clone(),
    ).map_err(|err| err.to_string())
}

#[tauri::command]
fn delete_game(id: i32, db: State<DbPool>, cmd_tx: State<CommandSender>) -> Result<(), String> {
    persistence::games::delete_game(id, db.inner().clone(), cmd_tx.inner().clone())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn restore_version(
    id: i32,
    db: State<DbPool>,
    cmd_tx: State<CommandSender>,
    globals: State<Globals>
) -> Result<(), String> {
    detection::restore_version(
        id,
        db.inner().clone(),
        cmd_tx.inner().clone(),
        &globals.working_base,
    ).map_err(|e| format!("{:?}", e))
}

pub fn add_handlers(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder.invoke_handler(generate_handler![
        get_games,
        get_game,
        save_game,
        delete_game,
        restore_version
    ])
}
