use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{generate_handler, Emitter, State};
use worldline_core::install_scanner::InstallScannerItem;
use worldline_core::models::*;
use worldline_core::WorldlineCore;

#[tauri::command]
fn get_games(core: State<Arc<WorldlineCore>>) -> Result<Vec<FullGame>, String> {
    core.get_games().map_err(|e| e.to_string())
}

#[tauri::command]
fn get_game(id: i32, core: State<Arc<WorldlineCore>>) -> Result<FullGame, String> {
    core.get_game(id).map_err(|e| e.to_string())
}

#[tauri::command]
fn save_game(
    game: GameForm,
    dirs: Option<Vec<String>>,
    core: State<Arc<WorldlineCore>>,
) -> Result<Game, String> {
    core.save_game(game, dirs).map_err(|err| err.to_string())
}

#[tauri::command]
fn delete_game(id: i32, core: State<Arc<WorldlineCore>>) -> Result<(), String> {
    core.delete_game(id).map_err(|e| e.to_string())
}

#[tauri::command]
async fn untrack_game(id: i32, core: State<'_, Arc<WorldlineCore>>) -> Result<(), String> {
    let core = Arc::clone(&core);
    tauri::async_runtime::spawn_blocking(move || core.untrack_game(id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn create_slot(
    game_id: i32,
    name: String,
    core: State<Arc<WorldlineCore>>,
) -> Result<FullGame, String> {
    core.create_slot(game_id, &name).map_err(|e| e.to_string())
}

#[tauri::command]
fn switch_slot(
    game_id: i32,
    slot_id: i32,
    core: State<Arc<WorldlineCore>>,
) -> Result<FullGame, String> {
    core.switch_slot(game_id, slot_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn rename_slot(
    slot_id: i32,
    name: String,
    core: State<Arc<WorldlineCore>>,
) -> Result<FullGame, String> {
    core.rename_slot(slot_id, &name).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_slot(
    slot_id: i32,
    replacement_slot_id: Option<i32>,
    core: State<Arc<WorldlineCore>>,
) -> Result<FullGame, String> {
    core.delete_slot(slot_id, replacement_slot_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn restore_version(id: i32, core: State<Arc<WorldlineCore>>) -> Result<(), String> {
    core.restore_version(id).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn tag_version(id: i32, tag: String, core: State<Arc<WorldlineCore>>) -> Result<Version, String> {
    core.tag_version(id, tag).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_game_icon(
    game_id: i32,
    source_path: String,
    core: State<Arc<WorldlineCore>>,
) -> Result<String, String> {
    core.set_game_icon(game_id, &source_path)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn set_game_accent(
    game_id: i32,
    hue: Option<u16>,
    core: State<Arc<WorldlineCore>>,
) -> Result<Game, String> {
    core.set_game_accent(game_id, hue)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn scan_installed(
    core: State<'_, Arc<WorldlineCore>>,
) -> Result<Vec<InstallScannerItem>, String> {
    let core = Arc::clone(&core);
    tauri::async_runtime::spawn_blocking(move || core.scan_installed())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_scan_roots(core: State<Arc<WorldlineCore>>) -> Result<Vec<String>, String> {
    core.get_scan_roots().map_err(|e| e.to_string())
}

#[tauri::command]
fn add_scan_root(path: String, core: State<Arc<WorldlineCore>>) -> Result<(), String> {
    core.add_scan_root(&path).map_err(|e| e.to_string())
}

#[tauri::command]
fn remove_scan_root(path: String, core: State<Arc<WorldlineCore>>) -> Result<(), String> {
    core.remove_scan_root(&path).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_monitoring_settings(core: State<Arc<WorldlineCore>>) -> Result<MonitoringSettings, String> {
    core.get_monitoring_settings().map_err(|e| e.to_string())
}

#[tauri::command]
fn update_monitoring_settings(
    monitoring_settings: MonitoringSettings,
    core: State<Arc<WorldlineCore>>,
) -> Result<MonitoringSettings, String> {
    core.update_monitoring_settings(monitoring_settings)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_device(core: State<Arc<WorldlineCore>>) -> worldline_core::sync::DeviceMetadata {
    core.device_metadata()
}

#[tauri::command]
fn update_device(
    name: String,
    kind: Option<worldline_core::sync::DeviceKind>,
    core: State<Arc<WorldlineCore>>,
) -> Result<worldline_core::sync::DeviceMetadata, String> {
    core.update_device(&name, kind).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_sync_settings(
    core: State<Arc<WorldlineCore>>,
) -> Result<worldline_core::sync::SyncSettings, String> {
    core.get_sync_settings().map_err(|e| e.to_string())
}

#[tauri::command]
async fn update_sync_settings(
    settings: worldline_core::sync::SyncSettings,
    core: State<'_, Arc<WorldlineCore>>,
) -> Result<worldline_core::sync::SyncSettings, String> {
    let core = Arc::clone(&core);
    tauri::async_runtime::spawn_blocking(move || core.update_sync_settings(settings))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
async fn refresh_sync(
    core: State<'_, Arc<WorldlineCore>>,
) -> Result<worldline_core::sync::SyncReport, String> {
    let core = Arc::clone(&core);
    tauri::async_runtime::spawn_blocking(move || core.refresh_sync())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn adopt_remote_game(
    game_uuid: String,
    core: State<'_, Arc<WorldlineCore>>,
) -> Result<FullGame, String> {
    let game_uuid = uuid::Uuid::parse_str(&game_uuid).map_err(|e| e.to_string())?;
    let core = Arc::clone(&core);
    tauri::async_runtime::spawn_blocking(move || core.adopt_remote_game(game_uuid))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn apply_remote_version(
    version_id: i32,
    core: State<'_, Arc<WorldlineCore>>,
) -> Result<(), String> {
    let core = Arc::clone(&core);
    tauri::async_runtime::spawn_blocking(move || core.apply_remote_version(version_id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn remove_sync_device(
    device_uuid: String,
    core: State<'_, Arc<WorldlineCore>>,
) -> Result<(), String> {
    let device_uuid = uuid::Uuid::parse_str(&device_uuid).map_err(|e| e.to_string())?;
    let core = Arc::clone(&core);
    tauri::async_runtime::spawn_blocking(move || core.remove_sync_device(device_uuid))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn get_retired_versions(core: State<Arc<WorldlineCore>>) -> Result<Vec<String>, String> {
    core.retired_versions().map_err(|e| e.to_string())
}

#[tauri::command]
fn get_pruning_settings(
    core: State<Arc<WorldlineCore>>,
) -> Result<worldline_core::maintenance::RetentionSettings, String> {
    core.get_pruning_settings().map_err(|e| e.to_string())
}

#[tauri::command]
fn update_pruning_settings(
    settings: worldline_core::maintenance::RetentionSettings,
    core: State<Arc<WorldlineCore>>,
) -> Result<worldline_core::maintenance::RetentionSettings, String> {
    core.update_pruning_settings(settings)
        .map_err(|e| e.to_string())
}

/// Previews `settings`, or the saved settings when none are given.
#[tauri::command]
async fn preview_pruning(
    settings: Option<worldline_core::maintenance::RetentionSettings>,
    core: State<'_, Arc<WorldlineCore>>,
) -> Result<worldline_core::core_facade::PruningPreview, String> {
    let core = Arc::clone(&core);
    tauri::async_runtime::spawn_blocking(move || {
        let settings = match settings {
            Some(settings) => settings,
            None => core.get_pruning_settings()?,
        };
        core.preview_pruning(&settings)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn run_pruning(
    core: State<'_, Arc<WorldlineCore>>,
    app: tauri::AppHandle,
) -> Result<worldline_core::core_facade::PruningRunReport, String> {
    let core = Arc::clone(&core);
    let report = tauri::async_runtime::spawn_blocking(move || core.run_pruning())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    if report.changed_history() {
        app.emit("history-pruned", ()).map_err(|e| e.to_string())?;
    }
    Ok(report)
}

#[tauri::command]
async fn inspect_history_bundle(
    bundle_path: String,
) -> Result<worldline_core::bundle::HistoryBundleSummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        WorldlineCore::inspect_history_bundle(std::path::Path::new(&bundle_path))
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn export_game_history(
    game_id: i32,
    destination: String,
    core: State<'_, Arc<WorldlineCore>>,
) -> Result<worldline_core::bundle::HistoryTransferReport, String> {
    let core = Arc::clone(&core);
    tauri::async_runtime::spawn_blocking(move || {
        core.export_game_history(game_id, std::path::Path::new(&destination))
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn import_game_history(
    bundle_path: String,
    mappings: HashMap<String, String>,
    core: State<'_, Arc<WorldlineCore>>,
) -> Result<worldline_core::bundle::HistoryTransferReport, String> {
    let mappings = mappings
        .into_iter()
        .map(|(uuid, path)| {
            uuid::Uuid::parse_str(&uuid)
                .map(|uuid| (uuid, PathBuf::from(path)))
                .map_err(|error| error.to_string())
        })
        .collect::<Result<HashMap<_, _>, _>>()?;
    let core = Arc::clone(&core);
    tauri::async_runtime::spawn_blocking(move || {
        core.import_game_history(std::path::Path::new(&bundle_path), &mappings)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

/// Where pre-release data was moved aside on startup, if it was; the GUI words the notice.
#[tauri::command]
fn get_startup_notice(core: State<Arc<WorldlineCore>>) -> Option<String> {
    core.moved_old_data()
        .map(|backup| backup.display().to_string())
}

#[tauri::command]
fn set_tray_labels(labels: crate::tray::TrayLabels, app: tauri::AppHandle) -> Result<(), String> {
    crate::tray::set_labels(&app, labels).map_err(|e| e.to_string())
}

pub fn add_handlers(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder.invoke_handler(generate_handler![
        get_games,
        get_game,
        save_game,
        delete_game,
        untrack_game,
        create_slot,
        switch_slot,
        rename_slot,
        delete_slot,
        restore_version,
        tag_version,
        set_game_icon,
        set_game_accent,
        scan_installed,
        get_scan_roots,
        add_scan_root,
        remove_scan_root,
        get_monitoring_settings,
        update_monitoring_settings,
        get_device,
        update_device,
        get_sync_settings,
        update_sync_settings,
        refresh_sync,
        adopt_remote_game,
        apply_remote_version,
        remove_sync_device,
        get_retired_versions,
        get_pruning_settings,
        update_pruning_settings,
        preview_pruning,
        run_pruning,
        inspect_history_bundle,
        export_game_history,
        import_game_history,
        get_startup_notice,
        set_tray_labels,
    ])
}
