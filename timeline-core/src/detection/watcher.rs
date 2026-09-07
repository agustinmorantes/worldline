use crate::detection::fs_event_handler::FsEventHandler;
use crate::detection::{Command, CommandReceiver, CommandSender, Notification, NotificationSender};
use crate::models::{FullGame, MonitoringSettings};
use crate::persistence::{GameRepository, VersionRepository};
use chrono::{NaiveDateTime, Utc};
use notify_debouncer_full::notify::{RecursiveMode, Watcher};
use notify_debouncer_full::{DebounceEventResult, Debouncer, FileIdMap, new_debouncer};
use std::collections::{HashMap, HashSet};
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

    let spawn_fs_listener = |fs_rx: mpsc::Receiver<DebounceEventResult>,
                             agg_tx: mpsc::Sender<AggregatedEvent>| {
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
            let shutdown = matches!(command, Command::Shutdown);
            match agg_tx_cmd.send(AggregatedEvent::Command(command)) {
                Ok(_) => {}
                Err(_) => break,
            }
            if shutdown {
                break;
            }
        }
    });

    // Filesystem event listener
    spawn_fs_listener(fs_rx, agg_tx.clone());

    // Global listener
    let mut paused = false;
    let mut deadlines = HashMap::new();
    'events: loop {
        let event = match next_timeout(&deadlines, paused, Utc::now().naive_utc()) {
            Some(timeout) => agg_rx.recv_timeout(timeout),
            None => agg_rx
                .recv()
                .map_err(|_| mpsc::RecvTimeoutError::Disconnected),
        };
        let event = match event {
            Ok(event) => event,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let now = Utc::now().naive_utc();
                let due: HashSet<_> = deadlines
                    .iter()
                    .filter_map(|(&id, &deadline)| (deadline <= now).then_some(id))
                    .collect();
                for id in due {
                    if !ctx.games.iter().any(|game| game.game.id == id) {
                        deadlines.remove(&id);
                        continue;
                    }
                    match fs_event_handler.process_modified_games(
                        &HashSet::from([id]),
                        ctx.notification_tx.clone(),
                        &ctx.games,
                        monitoring_settings,
                    ) {
                        Ok(result) => {
                            deadlines.remove(&id);
                            merge_deferred(&mut deadlines, result.deferred);
                            ctx.games = result.games;
                        }
                        Err(err) => {
                            if ctx
                                .notification_tx
                                .send(Notification::Error(format!(
                                    "FS event handler error: {}",
                                    err
                                )))
                                .is_err()
                            {
                                break 'events;
                            }
                            // Keep failed work, but avoid spinning on an expired deadline.
                            deadlines
                                .insert(id, Utc::now().naive_utc() + chrono::Duration::seconds(1));
                        }
                    }
                }
                continue;
            }
        };
        match event {
            AggregatedEvent::Command(cmd) => match cmd {
                Command::Shutdown => {
                    break;
                }
                Command::Pause(ack) => {
                    paused = true;
                    // Acknowledging here, from the same loop that dispatches
                    // filesystem events, is what makes the pause meaningful: the
                    // caller learns that every event queued ahead of this command
                    // has already been dispatched, and everything queued behind it
                    // will be dropped by the `paused` check below.
                    let _ = ack.send(());
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
                    recheck_deferred(&mut deadlines, &ctx.games);
                }
                Command::ReregisterGames(new_dirs) => {
                    unregister_games(&mut debouncer, &ctx.games);

                    ctx.games = new_dirs.clone();

                    register_games(&mut debouncer, &ctx.games);
                    recheck_deferred(&mut deadlines, &ctx.games);
                }
                Command::UpdateMonitoringSettings(new_settings) => {
                    let debounce_changed = (monitoring_settings.snapshot_debounce_seconds
                        - new_settings.snapshot_debounce_seconds)
                        .abs()
                        > f32::EPSILON;
                    monitoring_settings = new_settings;
                    recheck_deferred(&mut deadlines, &ctx.games);

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
                            merge_deferred(&mut deadlines, updated_games.deferred);
                            ctx.games = updated_games.games;
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

fn merge_deferred(
    deadlines: &mut HashMap<i32, NaiveDateTime>,
    deferred: Vec<(i32, NaiveDateTime)>,
) {
    for (id, eligible_at) in deferred {
        deadlines
            .entry(id)
            .and_modify(|deadline| *deadline = (*deadline).max(eligible_at))
            .or_insert(eligible_at);
    }
}

fn recheck_deferred(deadlines: &mut HashMap<i32, NaiveDateTime>, games: &[FullGame]) {
    let now = Utc::now().naive_utc();
    deadlines.retain(|id, deadline| {
        *deadline = now;
        games.iter().any(|game| game.game.id == *id)
    });
}

fn next_timeout(
    deadlines: &HashMap<i32, NaiveDateTime>,
    paused: bool,
    now: NaiveDateTime,
) -> Option<Duration> {
    if paused {
        return None;
    }
    deadlines
        .values()
        .min()
        .map(|deadline| (*deadline - now).to_std().unwrap_or_default())
}

enum AggregatedEvent {
    Command(Command),
    FsEvent(DebounceEventResult),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{FullVersion, Game, Slot, TrackedDir, Version};
    use crate::persistence::{MockGameRepository, MockVersionRepository};
    use chrono::Utc;
    use tempfile::tempdir;

    #[test]
    fn deadlines_coalesce_to_later_eligibility_and_pause_disables_timeouts() {
        let now = Utc::now().naive_utc();
        let mut deadlines = HashMap::new();
        merge_deferred(
            &mut deadlines,
            vec![(1, now + chrono::Duration::seconds(5))],
        );
        merge_deferred(
            &mut deadlines,
            vec![
                (1, now + chrono::Duration::seconds(3)),
                (2, now + chrono::Duration::seconds(4)),
            ],
        );
        assert_eq!(deadlines.len(), 2);
        assert_eq!(deadlines[&1], now + chrono::Duration::seconds(5));
        assert_eq!(
            next_timeout(&deadlines, false, now),
            Some(Duration::from_secs(4))
        );
        merge_deferred(
            &mut deadlines,
            vec![(1, now + chrono::Duration::seconds(8))],
        );
        assert_eq!(deadlines[&1], now + chrono::Duration::seconds(8));
        assert_eq!(
            next_timeout(&deadlines, true, now + chrono::Duration::seconds(10)),
            None
        );
        assert_eq!(
            next_timeout(&deadlines, false, now + chrono::Duration::seconds(10)),
            Some(Duration::ZERO)
        );
        assert_eq!(next_timeout(&HashMap::new(), false, now), None);
        recheck_deferred(&mut deadlines, &[]);
        assert!(
            deadlines.is_empty(),
            "unregistered games must lose their deadlines"
        );
    }

    #[test]
    fn deferred_capture_survives_pause_and_retries_after_resume_without_an_event() {
        assert_deferred_capture(true);
    }

    #[test]
    fn deferred_capture_retries_on_timeout_and_shutdown_does_not_wait_for_pending_work() {
        assert_deferred_capture(false);
    }

    fn assert_deferred_capture(pause: bool) {
        let working_base = tempdir().unwrap();
        let source = tempdir().unwrap();
        let now = Utc::now().naive_utc();
        let slot = Slot {
            id: 10,
            uuid: uuid::Uuid::new_v4().to_string(),
            game_id: 1,
            name: "Default".into(),
            name_normalized: "default".into(),
            folder_slug: "default-slot".into(),
            active_version_id: Some(1),
            created_at: now,
            updated_at: now,
        };
        let version = Version {
            id: 1,
            uuid: None,
            folder_slug: None,
            slot_id: 10,
            version_number: 1,
            tag: None,
            reason: "watcher".into(),
            created_at: now,
            parent_id: None,
        };
        let game = FullGame {
            game: Game {
                id: 1,
                uuid: None,
                title: "Test".into(),
                title_normalized: None,
                folder_slug: None,
                active_slot_id: Some(10),
                icon_path: None,
            },
            slots: vec![slot.clone()],
            active_slot: Some(slot.clone()),
            last_version: Some(version.clone()),
            versions: vec![version],
            tracked_dirs: vec![TrackedDir {
                id: 1,
                uuid: Some(uuid::Uuid::new_v4().to_string()),
                folder_slug: None,
                game_id: 1,
                path: source.path().to_string_lossy().into_owned(),
                exclusions: "[]".into(),
            }],
        };
        let created = FullVersion {
            version: Version {
                id: 2,
                version_number: 2,
                ..game.versions[0].clone()
            },
            game: game.game.clone(),
            slot,
            parent_uuid: None,
        };
        let mut refreshed = game.clone();
        refreshed.versions.push(created.version.clone());
        refreshed.last_version = Some(created.version.clone());
        let mut games_repo = MockGameRepository::new();
        games_repo
            .expect_get_games()
            .times(1)
            .returning(move || Ok(vec![refreshed.clone()]));
        let mut versions_repo = MockVersionRepository::new();
        versions_repo
            .expect_create_version_from_snapshot()
            .times(1)
            .returning(move |id, reason, snapshot| {
                assert_eq!(id, 1);
                assert_eq!(reason, crate::repository::VersionReason::Watcher);
                assert_eq!(snapshot.files.len(), 1);
                Ok(created.clone())
            });
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (notification_tx, notification_rx) = mpsc::channel();
        let thread_tx = cmd_tx.clone();
        let initial_game = game.clone();
        let (done_tx, done_rx) = mpsc::channel();
        let watcher = std::thread::spawn(move || {
            watch(WatcherContext {
                cmd_rx,
                _cmd_tx: thread_tx,
                notification_tx,
                games: vec![initial_game],
                working_base: working_base.path(),
                games_repo: Arc::new(games_repo),
                versions_repo: Arc::new(versions_repo),
                monitoring_settings: MonitoringSettings {
                    snapshot_debounce_seconds: 0.01,
                    version_cooldown_minutes: 1,
                },
            });
            let _ = done_tx.send(());
        });
        // Repeat only until the OS watcher has registered and reports the first change.
        let mut detected = false;
        for i in 0..30 {
            std::fs::write(source.path().join("save.dat"), i.to_string()).unwrap();
            if matches!(
                notification_rx.recv_timeout(Duration::from_millis(100)),
                Ok(Notification::ChangeDetected { game_id: 1 })
            ) {
                detected = true;
                break;
            }
        }
        if pause {
            let (ack_tx, ack_rx) = mpsc::channel();
            cmd_tx.send(Command::Pause(ack_tx)).unwrap();
            ack_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        }
        // Re-registration changes the deadline; no filesystem event follows.
        let mut eligible_soon = game;
        eligible_soon.versions[0].created_at =
            Utc::now().naive_utc() - chrono::Duration::seconds(59);
        cmd_tx
            .send(Command::ReregisterGames(vec![eligible_soon]))
            .unwrap();
        let paused_messages: Vec<_> = notification_rx.try_iter().collect();
        let while_paused = if pause {
            let result = notification_rx.recv_timeout(Duration::from_millis(1100));
            cmd_tx.send(Command::Resume).unwrap();
            result
        } else {
            Err(mpsc::RecvTimeoutError::Timeout)
        };
        let after_resume = notification_rx.recv_timeout(Duration::from_secs(3));
        let extra_messages: Vec<_> = notification_rx.try_iter().collect();
        // Queue another cooldown-blocked change before shutting down.
        std::fs::write(source.path().join("save.dat"), b"pending at shutdown").unwrap();
        let pending_change = notification_rx.recv_timeout(Duration::from_secs(2));
        cmd_tx.send(Command::Shutdown).unwrap();
        let stopped = done_rx.recv_timeout(Duration::from_secs(2));
        let joined = stopped.is_ok().then(|| watcher.join());
        assert!(detected, "watcher never detected the source change");
        assert!(
            paused_messages
                .iter()
                .all(|n| matches!(n, Notification::ChangeDetected { .. }))
        );
        assert!(matches!(while_paused, Err(mpsc::RecvTimeoutError::Timeout)));
        assert!(matches!(
            after_resume,
            Ok(Notification::VersionCreated {
                game_id: 1,
                version_number: 2
            })
        ));
        assert!(
            extra_messages.is_empty(),
            "retry must not emit another ChangeDetected"
        );
        assert!(matches!(
            pending_change,
            Ok(Notification::ChangeDetected { game_id: 1 })
        ));
        assert!(stopped.is_ok(), "shutdown must not wait for a cooldown");
        assert!(matches!(joined, Some(Ok(()))));
    }
}
