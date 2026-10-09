//! Native dialogs, behind commands so the `e2e` build can stub them: pickers
//! return what the test wrote to `dialog-answer` in the data dir, and confirmations accept.

use serde::Deserialize;
use tauri::AppHandle;
#[cfg(not(feature = "e2e"))]
use tauri_plugin_dialog::{DialogExt, FilePath, MessageDialogButtons, MessageDialogKind};

#[derive(Deserialize)]
#[cfg_attr(feature = "e2e", allow(dead_code))]
pub struct FileFilter {
    name: String,
    extensions: Vec<String>,
}

#[tauri::command]
pub async fn pick_folder(title: String, app: AppHandle) -> Result<Option<String>, String> {
    #[cfg(feature = "e2e")]
    {
        let _ = (title, app);
        Ok(stub_path())
    }
    #[cfg(not(feature = "e2e"))]
    picked(move || app.dialog().file().set_title(title).blocking_pick_folder()).await
}

#[tauri::command]
pub async fn pick_file(
    title: String,
    filters: Vec<FileFilter>,
    app: AppHandle,
) -> Result<Option<String>, String> {
    #[cfg(feature = "e2e")]
    {
        let _ = (title, filters, app);
        Ok(stub_path())
    }
    #[cfg(not(feature = "e2e"))]
    picked(move || {
        let mut dialog = app.dialog().file().set_title(title);
        for filter in &filters {
            let extensions: Vec<&str> = filter.extensions.iter().map(String::as_str).collect();
            dialog = dialog.add_filter(&filter.name, &extensions);
        }
        dialog.blocking_pick_file()
    })
    .await
}

#[tauri::command]
pub async fn pick_save_path(
    title: String,
    file_name: String,
    filters: Vec<FileFilter>,
    app: AppHandle,
) -> Result<Option<String>, String> {
    #[cfg(feature = "e2e")]
    {
        let _ = (title, file_name, filters, app);
        Ok(stub_path())
    }
    #[cfg(not(feature = "e2e"))]
    picked(move || {
        let mut dialog = app
            .dialog()
            .file()
            .set_title(title)
            .set_file_name(file_name);
        for filter in &filters {
            let extensions: Vec<&str> = filter.extensions.iter().map(String::as_str).collect();
            dialog = dialog.add_filter(&filter.name, &extensions);
        }
        dialog.blocking_save_file()
    })
    .await
}

#[tauri::command]
pub async fn confirm(title: String, message: String, app: AppHandle) -> Result<bool, String> {
    #[cfg(feature = "e2e")]
    {
        let _ = (title, message, app);
        Ok(true)
    }
    #[cfg(not(feature = "e2e"))]
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .message(message)
            .title(title)
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::OkCancel)
            .blocking_show()
    })
    .await
    .map_err(|e| e.to_string())
}

#[cfg(not(feature = "e2e"))]
async fn picked(
    pick: impl FnOnce() -> Option<FilePath> + Send + 'static,
) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || pick().map(|path| path.to_string()))
        .await
        .map_err(|e| e.to_string())
}

/// The file's contents in `$WORLDLINE_E2E_DATA_DIR/dialog-answer`, so each test can
/// answer the next picker; no file means the picker was cancelled.
#[cfg(feature = "e2e")]
fn stub_path() -> Option<String> {
    let dir = std::env::var_os("WORLDLINE_E2E_DATA_DIR")?;
    let answer = std::fs::read_to_string(std::path::Path::new(&dir).join("dialog-answer")).ok()?;
    Some(answer.trim().to_owned())
}
