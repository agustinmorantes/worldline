use savegame_timeline_core::models::*;
use savegame_timeline_core::install_scanner::InstallScannerItem;
use savegame_timeline_core::TimelineCore;
use anyhow::Result;
use std::sync::Arc;
use tauri::{generate_handler, State};

#[tauri::command]
fn get_games(core: State<Arc<TimelineCore>>) -> Result<Vec<FullGame>, String> {
    core.get_games().map_err(|e| e.to_string())
}

#[tauri::command]
fn get_game(id: i32, core: State<Arc<TimelineCore>>) -> Result<FullGame, String> {
    core.get_game(id).map_err(|e| e.to_string())
}

#[tauri::command]
fn save_game(
    game: GameForm,
    dirs: Option<Vec<&str>>,
    core: State<Arc<TimelineCore>>,
) -> Result<Game, String> {
    core.save_game(game, dirs).map_err(|err| err.to_string())
}

#[tauri::command]
fn delete_game(id: i32, core: State<Arc<TimelineCore>>) -> Result<(), String> {
    core.delete_game(id).map_err(|e| e.to_string())
}

#[tauri::command]
fn restore_version(
    id: i32,
    core: State<Arc<TimelineCore>>,
) -> Result<(), String> {
    core.restore_version(id).map_err(|e| format!("{:?}", e))
}

#[tauri::command]
fn tag_version(
    id: i32,
    tag: String,
    core: State<Arc<TimelineCore>>,
) -> Result<Version, String> {
    core.tag_version(id, tag).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_game_icon(
    game_id: i32,
    source_path: String,
    core: State<Arc<TimelineCore>>,
) -> Result<String, String> {
    core.set_game_icon(game_id, &source_path).map_err(|e| e.to_string())
}

#[tauri::command]
fn scan_installed() -> Vec<InstallScannerItem> {
    savegame_timeline_core::install_scanner::scan_installed()
}

pub fn add_handlers(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder.invoke_handler(generate_handler![
        get_games,
        get_game,
        save_game,
        delete_game,
        restore_version,
        tag_version,
        set_game_icon,
        scan_installed,
    ])
}
