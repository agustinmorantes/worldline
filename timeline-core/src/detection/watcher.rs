use crate::detection::fs_event_handler::FsEventHandler;
use crate::detection::{Command, CommandReceiver, CommandSender, Notification, NotificationSender};
use crate::models::{FullGame, MonitoringSettings};
use crate::persistence::{GameRepository, VersionRepository};
use notify_debouncer_full::notify::{RecursiveMode, Watcher};
use notify_debouncer_full::{DebounceEventResult, Debouncer, FileIdMap, new_debouncer};
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

pub struct WatcherContext<'a> {
    pub cmd_rx: CommandReceiver,
    pub _cmd_tx: CommandSender,
    pub notification_tx: NotificationSender,
    pub games: Vec<FullGame>,
    pub working_base: &'a Path,
    pub games_repo: Arc<dyn GameRepository>,
    pub versions_repo: Arc<dyn VersionRepository>,
    pub monitoring_settings: MonitoringSettings,
}

type RecommendedDebouncer = Debouncer<notify_debouncer_full::notify::RecommendedWatcher, FileIdMap>;

pub fn watch(mut ctx: WatcherContext) {
    let create_debouncer = |debounce_seconds: f32| {
        let (fs_tx, fs_rx) = mpsc::channel();
        let debouncer = new_debouncer(Duration::from_secs_f32(debounce_seconds), None, fs_tx)
            .expect("couldn't create watcher");
        (debouncer, fs_rx)
    };

    let spawn_fs_listener =
        |fs_rx: mpsc::Receiver<DebounceEventResult>, agg_tx: mpsc::Sender<AggregatedEvent>| {
            std::thread::spawn(move || {
                for event in fs_rx {
                    match agg_tx.send(AggregatedEvent::FsEvent(event)) {
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
            });
        };

    let register_games = |debouncer: &mut RecommendedDebouncer, games: &[FullGame]| {
        for game in games {
            for dir in &game.tracked_dirs {
                let res = debouncer
                    .watcher()
                    .watch(Path::new(&dir.path), RecursiveMode::Recursive)
                    .map_err(|err| print!("Error watching '{}': {}", dir.path, err));

                if res.is_ok() {
                    println!("Watching '{}'", dir.path);
                }
            }
        }
    };

    let unregister_games = |debouncer: &mut RecommendedDebouncer, games: &[FullGame]| {
        for game in games {
            for dir in &game.tracked_dirs {
                debouncer
                    .watcher()
                    .unwatch(Path::new(&dir.path))
                    .map_err(|err| println!("Error unwatching '{}': {}", dir.path, err))
                    .ok();
            }
        }
    };

    let mut monitoring_settings = ctx.monitoring_settings;
    let (mut debouncer, fs_rx) = create_debouncer(monitoring_settings.snapshot_debounce_seconds);
    register_games(&mut debouncer, &ctx.games);

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
    spawn_fs_listener(fs_rx, agg_tx.clone());

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
                    // Recreate debouncer to discard events buffered during the restore file copy.
                    // The copy fires OS events that the debouncer captures; dropping it before
                    // the debounce window expires prevents those from triggering a spurious version.
                    unregister_games(&mut debouncer, &ctx.games);
                    let (new_debouncer, new_fs_rx) =
                        create_debouncer(monitoring_settings.snapshot_debounce_seconds);
                    debouncer = new_debouncer;
                    register_games(&mut debouncer, &ctx.games);
                    spawn_fs_listener(new_fs_rx, agg_tx.clone());
                }
                Command::ReregisterGames(new_dirs) => {
                    unregister_games(&mut debouncer, &ctx.games);

                    ctx.games = new_dirs.clone();

                    register_games(&mut debouncer, &ctx.games);
                }
                Command::UpdateMonitoringSettings(new_settings) => {
                    let debounce_changed = (monitoring_settings.snapshot_debounce_seconds
                        - new_settings.snapshot_debounce_seconds)
                        .abs()
                        > f32::EPSILON;
                    monitoring_settings = new_settings;

                    if debounce_changed {
                        unregister_games(&mut debouncer, &ctx.games);
                        let (new_debouncer, new_fs_rx) =
                            create_debouncer(monitoring_settings.snapshot_debounce_seconds);
                        debouncer = new_debouncer;
                        register_games(&mut debouncer, &ctx.games);
                        spawn_fs_listener(new_fs_rx, agg_tx.clone());
                    }
                }
            },
            AggregatedEvent::FsEvent(res) => match res {
                Ok(event) => {
                    if paused {
                        continue;
                    }

                    match fs_event_handler.run(
                        event,
                        ctx.notification_tx.clone(),
                        &ctx.games,
                        ctx.working_base,
                        monitoring_settings,
                    ) {
                        Ok(updated_games) => {
                            ctx.games = updated_games;
                        }
                        Err(err) => {
                            match ctx.notification_tx.send(Notification::Error(format!(
                                "FS event handler error: {}",
                                err
                            ))) {
                                Ok(_) => {}
                                Err(_) => break,
                            }
                        }
                    }
                }
                Err(err) => {
                    match ctx
                        .notification_tx
                        .send(Notification::Error(format!("Watcher error: {:?}", err)))
                    {
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
