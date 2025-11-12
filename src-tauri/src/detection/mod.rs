mod watcher;
mod fs_event_handler;
mod restore;

pub use restore::*;

use crate::models::FullGame;
use crate::DbPool;
use anyhow::{Context, Result};
use std::sync::mpsc;
use tauri::{AppHandle, Manager};
use crate::persistence::games::get_games;

pub fn init(app: AppHandle) -> Result<(CommandSender, NotificationReceiver)> {
    let (command_tx, command_rx): (CommandSender, CommandReceiver) = mpsc::channel();
    let (notification_tx, notification_rx): (NotificationSender, NotificationReceiver) = mpsc::channel();

    let db = app.state::<DbPool>();
    let games = get_games(db.inner().clone())
        .context("failed to get games")?;
    
    let app_local = app.path().app_local_data_dir()
        .context("failed to get app data directory")?;
    let working_base = app_local.join("repository");

    let db_clone = db.inner().clone();
    let cmd_tx_clone = command_tx.clone();
    std::thread::spawn(move || {
        watcher::watch(command_rx, cmd_tx_clone, notification_tx, games, &working_base, db_clone);
    });

    Ok((command_tx, notification_rx))
}

pub fn reregister_games(command_tx: &CommandSender, db: &DbPool) -> Result<()> {
    let games = get_games(db.clone())?;

    command_tx
        .send(Command::ReregisterGames(games))
        .context("Error sending ReregisterGames command")
}

type CommandReceiver = mpsc::Receiver<Command>;
type NotificationSender = mpsc::Sender<Notification>;

pub type CommandSender = mpsc::Sender<Command>;
pub type NotificationReceiver = mpsc::Receiver<Notification>;

pub enum Command {
    Pause,
    Resume,
    ReregisterGames(Vec<FullGame>),
    Shutdown,
}

pub enum Notification {
    ChangeDetected{ game_id: i32 },
    VersionCreated{ game_id: i32, version_number: i32 },
    Error(String),
}
