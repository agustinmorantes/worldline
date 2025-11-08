mod watcher;
mod tracked_dirs;
mod fs_event_handler;

use crate::models::TrackedDir;
use crate::DbPool;
use anyhow::Result;
use std::sync::mpsc;
use tauri::{AppHandle, Manager};

pub fn init(app: AppHandle) -> Result<(CommandSender, NotificationReceiver)> {
    let (command_tx, command_rx): (CommandSender, CommandReceiver) = mpsc::channel();
    let (notification_tx, notification_rx): (NotificationSender, NotificationReceiver) = mpsc::channel();

    let db = app.state();
    let tracked_dirs = tracked_dirs::get_tracked_dirs(&db);
    let app_local = app.path().app_local_data_dir()
        .expect("failed to get app data directory");

    let working_base = app_local.join("detection_working");


    std::thread::spawn(move || {
        watcher::watch(command_rx, notification_tx, tracked_dirs, &working_base);
    });

    Ok((command_tx, notification_rx))
}

pub fn reregister_dirs(command_tx: &CommandSender, db: &DbPool) -> Result<()> {
    let dirs = tracked_dirs::get_tracked_dirs(db);

    command_tx
        .send(Command::ReregisterDirs(dirs))
        .map_err(|e| anyhow::anyhow!("Error sending ReregisterDirs command: {}", e))
}

type CommandReceiver = mpsc::Receiver<Command>;
type NotificationSender = mpsc::Sender<Notification>;

pub type CommandSender = mpsc::Sender<Command>;
pub type NotificationReceiver = mpsc::Receiver<Notification>;

pub enum Command {
    Pause,
    Resume,
    ReregisterDirs(Vec<TrackedDir>),
    Shutdown,
}

pub enum Notification {
    ChangeDetected{ game_id: i32 },
    VersionCreated{ game_id: i32, version_id: i32 },
    Error(String),
}
