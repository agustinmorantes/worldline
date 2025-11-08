use crate::detection::fs_event_handler::fs_event_handler;
use crate::detection::{Command, CommandReceiver, Notification, NotificationSender};
use crate::models::TrackedDir;
use notify::{RecursiveMode};
use notify_debouncer_full::{new_debouncer, DebounceEventResult};
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

pub fn watch(
    command_rx: CommandReceiver,
    notification_tx: NotificationSender,
    mut dirs: Vec<TrackedDir>,
    working_base: &Path,
) {
    let (fs_tx, fs_rx) = mpsc::channel();

    let mut debouncer = new_debouncer(
        Duration::from_secs(2),
        None,
        fs_tx
    ).expect("couldn't create watcher");

    for dir in &dirs {
        let res = debouncer
            .watch(Path::new(&dir.path), RecursiveMode::Recursive)
            .map_err(|err| print!("Error watching '{}': {}", dir.path, err));

        if res.is_ok() {
            println!("Watching '{}'", dir.path);
        }
    }

    // Aggregated event channel
    let (agg_tx, agg_rx) = mpsc::channel();

    // Command listener
    let agg_tx_cmd = agg_tx.clone();
    std::thread::spawn(move || {
        for command in command_rx {
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
                Command::ReregisterDirs(new_dirs) => {
                    for dir in &dirs {
                        debouncer
                            .unwatch(Path::new(&dir.path))
                            .map_err(|err| println!("Error unwatching '{}': {}", dir.path, err))
                            .ok();
                    }

                    dirs = new_dirs.clone();

                    for dir in &dirs {
                        let res = debouncer
                            .watch(Path::new(&dir.path), RecursiveMode::Recursive)
                            .map_err(|err| println!("Error watching '{}': {}", dir.path, err));

                        if res.is_ok() {
                            println!("Watching '{}'", dir.path);
                        }
                    }
                }
            },
            AggregatedEvent::FsEvent(res) => match res {
                Ok(event) => {
                    if paused {
                        continue;
                    }
                    
                    match fs_event_handler(event, notification_tx.clone(), &dirs, working_base) {
                        Ok(_) => {}
                        Err(err) => {
                            match notification_tx
                                .send(Notification::Error(format!("FS event handler error: {}", err))) {
                                Ok(_) => {}
                                Err(_) => break,
                            }
                        }
                    }
                }
                Err(err) => {
                    match notification_tx
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
