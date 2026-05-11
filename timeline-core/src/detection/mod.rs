mod fs_event_handler;
mod watcher;

use crate::models::FullGame;
use crate::persistence::{GameRepository, VersionRepository};
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;

pub fn init(
    games_repo: Arc<dyn GameRepository>,
    versions_repo: Arc<dyn VersionRepository>,
    working_base: PathBuf,
    new_version_seconds: f32,
) -> Result<(CommandSender, NotificationReceiver)> {
    let (command_tx, command_rx): (CommandSender, CommandReceiver) = mpsc::channel();
    let (notification_tx, notification_rx): (NotificationSender, NotificationReceiver) =
        mpsc::channel();

    let games = games_repo.get_games().context("failed to get games")?;

    let cmd_tx_clone = command_tx.clone();

    std::thread::spawn(move || {
        watcher::watch(watcher::WatcherContext {
            cmd_rx: command_rx,
            _cmd_tx: cmd_tx_clone,
            notification_tx,
            games,
            working_base: &working_base,
            games_repo,
            versions_repo,
            new_version_seconds,
        });
    });

    Ok((command_tx, notification_rx))
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
    ChangeDetected { game_id: i32 },
    VersionCreated { game_id: i32, version_number: i32 },
    Error(String),
}
