use crate::detection::fs_event_handler::FsEventHandler;
use crate::detection::{Command, CommandReceiver, CommandSender, Notification, NotificationSender};
use crate::models::FullGame;
use crate::persistence::{GameRepository, VersionRepository};
use notify_debouncer_full::notify::{RecursiveMode, Watcher};
use notify_debouncer_full::{new_debouncer, DebounceEventResult};
use std::path::Path;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

pub struct WatcherContext<'a> {
    pub cmd_rx: CommandReceiver,
    pub _cmd_tx: CommandSender,
    pub notification_tx: NotificationSender,
    pub games: Vec<FullGame>,
    pub working_base: &'a Path,
    pub games_repo: Arc<dyn GameRepository>,
    pub versions_repo: Arc<dyn VersionRepository>,
    pub new_version_seconds: f32,
}

pub fn watch(mut ctx: WatcherContext) {
    let (fs_tx, fs_rx) = mpsc::channel();

    let mut debouncer = new_debouncer(
        Duration::from_secs_f32(ctx.new_version_seconds),
        None,
        fs_tx
    ).expect("couldn't create watcher");

    for game in &ctx.games {
        for dir in &game.tracked_dirs {
            let res = debouncer.watcher()
                .watch(Path::new(&dir.path), RecursiveMode::Recursive)
                .map_err(|err| print!("Error watching '{}': {}", dir.path, err));

            if res.is_ok() {
                println!("Watching '{}'", dir.path);
            }
        }
    }

    let fs_event_handler = FsEventHandler::new(
        ctx.working_base.to_path_buf(),
        ctx.games_repo,
        ctx.versions_repo,
    );

    // Aggregated event channel
    let (agg_tx, agg_rx) = mpsc::channel();

    // Command listener
    let agg_tx_cmd = agg_tx.clone();
    std::thread::spawn(move || {
        for command in ctx.cmd_rx {
            match agg_tx_cmd.send(AggregatedEvent::Command(command)) {
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    // Filesystem event listener
    let agg_tx_fs = agg_tx.clone();
    std::thread::spawn(move || {
        for event in fs_rx {
            match agg_tx_fs.send(AggregatedEvent::FsEvent(event)) {
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    // Global listener
    let mut paused = false;
    for event in agg_rx {
        match event {
            AggregatedEvent::Command(cmd) => match cmd {
                Command::Shutdown => {
                    break;
                }
                Command::Pause => {
                    paused = true;
                }
                Command::Resume => {
                    paused = false;
                }
                Command::ReregisterGames(new_dirs) => {
                    for game in &ctx.games {
                        for dir in &game.tracked_dirs {
                            debouncer.watcher()
                                .unwatch(Path::new(&dir.path))
                                .map_err(|err| println!("Error unwatching '{}': {}", dir.path, err))
                                .ok();
                        }
                    }

                    ctx.games = new_dirs.clone();

                    for game in &ctx.games {
                        for dir in &game.tracked_dirs {
                            let res = debouncer.watcher()
                                .watch(Path::new(&dir.path), RecursiveMode::Recursive)
                                .map_err(|err| println!("Error watching '{}': {}", dir.path, err));

                            if res.is_ok() {
                                println!("Watching '{}'", dir.path);
                            }
                        }
                    }
                }
            },
            AggregatedEvent::FsEvent(res) => match res {
                Ok(event) => {
                    if paused {
                        continue;
                    }
                    
                    match fs_event_handler.run(event, ctx.notification_tx.clone(), &ctx.games, ctx.working_base) {
                        Ok(_) => {}
                        Err(err) => {
                            match ctx.notification_tx
                                .send(Notification::Error(format!("FS event handler error: {}", err))) {
                                Ok(_) => {}
                                Err(_) => break,
                            }
                        }
                    }
                }
                Err(err) => {
                    match ctx.notification_tx
                        .send(Notification::Error(format!("Watcher error: {:?}", err))) {
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
            },
        }
    }
}

enum AggregatedEvent {
    Command(Command),
    FsEvent(DebounceEventResult),
}
