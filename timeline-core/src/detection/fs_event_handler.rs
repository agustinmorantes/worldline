use crate::detection::{Notification, NotificationSender};
use crate::models::{FullGame, FullVersion, MonitoringSettings, TrackedDir};
use crate::persistence::{GameRepository, VersionRepository};
use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use notify_debouncer_full::DebouncedEvent;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct FsEventHandler {
    working_base: PathBuf,
    games_repo: Arc<dyn GameRepository>,
    versions_repo: Arc<dyn VersionRepository>,
}

impl FsEventHandler {
    pub fn new(
        working_base: PathBuf,
        games_repo: Arc<dyn GameRepository>,
        versions_repo: Arc<dyn VersionRepository>,
    ) -> Self {
        Self {
            working_base,
            games_repo,
            versions_repo,
        }
    }

    pub fn run(
        &self,
        events: Vec<DebouncedEvent>,
        notif_tx: NotificationSender,
        games: &[FullGame],
        working_base: &Path,
        monitoring_settings: MonitoringSettings,
    ) -> Result<Vec<FullGame>> {
        let games = games.to_vec();

        let dirs = games
            .iter()
            .flat_map(|game| game.tracked_dirs.iter())
            .cloned()
            .collect::<Vec<TrackedDir>>();

        let first_scan_dirs = games
            .iter()
            .flat_map(|game| game.tracked_dirs.iter().map(move |dir| (game, dir)))
            .filter(|(game, dir)| {
                let working_dir_path = &working_base
                    .join(game.game.id.to_string())
                    .join(game.game.active_version_id.unwrap_or(1).to_string())
                    .join(dir.id.to_string());
                !working_dir_path.exists()
            })
            .map(|(_, dir)| dir)
            .cloned()
            .collect::<Vec<TrackedDir>>();

        self.handle_rescan(&first_scan_dirs, &games).ok();

        // Collect all modified tracked directories
        let mut modified_dirs: HashSet<i32> = HashSet::new();
        let mut modified_games: HashSet<i32> = HashSet::new();

        for ev in events {
            println!("event detected: {:?}", ev);

            if ev.need_rescan() {
                self.handle_rescan(&dirs, &games)?;
                return Ok(games);
            }

            // Find which tracked directories were affected by this event
            for path in &ev.paths {
                if let Some(name) = path.file_name().and_then(|n| n.to_str())
                    && crate::fs_utils::is_excluded_file(name)
                {
                    continue;
                }

                for dir in &dirs {
                    if path.starts_with(&dir.path) {
                        modified_dirs.insert(dir.id);
                        modified_games.insert(dir.game_id);
                    }
                }
            }
        }

        // Create new versions for modified games BEFORE syncing
        // This ensures that new changes go into the new version directory
        let mut new_versions: HashSet<i32> = HashSet::new();
        for game_id in &modified_games {
            let game = games
                .iter()
                .find(|g| g.game.id == *game_id)
                .context("failed to find modified game")?;

            if let Some(version) = self
                .handle_new_version(game, monitoring_settings)
                .context("failed to handle new version")?
            {
                new_versions.insert(*game_id);

                // Send notification about new version
                notif_tx.send(Notification::VersionCreated {
                    game_id: *game_id,
                    version_number: version.version.version_number,
                })?;
            }
        }

        // Reload games if new versions were created to get updated active_version_id
        let games = if !new_versions.is_empty() {
            self.reload_games(&games)?
        } else {
            games
        };

        // Now copy all modified tracked directories into the correct version
        let dirs_to_sync: Vec<&TrackedDir> = dirs
            .iter()
            .filter(|dir| modified_dirs.contains(&dir.id))
            .collect();

        for dir in &dirs_to_sync {
            self.sync_tracked_dir(dir, &games)
                .unwrap_or_else(|e| eprintln!("Error syncing tracked dir {}: {}", dir.id, e));
        }

        // Send change detected notifications
        for game_id in &modified_games {
            notif_tx.send(Notification::ChangeDetected { game_id: *game_id })?;
        }

        Ok(games)
    }

    fn sync_tracked_dir(&self, dir: &TrackedDir, games: &[FullGame]) -> Result<()> {
        let Some(game) = games.iter().find(|g| g.game.id == dir.game_id) else {
            return Ok(());
        };

        let source_path = Path::new(&dir.path);
        let working_dir_path = &self
            .working_base
            .join(game.game.id.to_string())
            .join(game.game.active_version_id.unwrap_or(1).to_string())
            .join(dir.id.to_string());

        // Remove the existing directory if it exists
        if working_dir_path.exists() {
            fs::remove_dir_all(working_dir_path)?;
        }

        // Ensure parent directory exists
        if let Some(parent) = working_dir_path.parent()
            && !parent.exists()
        {
            fs::create_dir_all(parent)?;
        }

        crate::fs_utils::copy_dir_filtered(source_path, working_dir_path)?;

        Ok(())
    }

    fn handle_rescan(&self, dirs: &[TrackedDir], games: &[FullGame]) -> Result<()> {
        for dir in dirs {
            let Some(game) = games.iter().find(|g| g.game.id == dir.game_id) else {
                continue;
            };

            let path = Path::new(&dir.path);
            let working_dir_path = &self
                .working_base
                .join(game.game.id.to_string())
                .join(game.game.active_version_id.unwrap_or(1).to_string())
                .join(dir.id.to_string());

            if working_dir_path.exists() {
                fs::remove_dir_all(working_dir_path)?;
            } else if !working_dir_path.parent().unwrap().exists() {
                fs::create_dir_all(working_dir_path.parent().unwrap())?;
            }

            crate::fs_utils::copy_dir_filtered(path, working_dir_path)?;
        }

        Ok(())
    }

    fn reload_games(&self, _old_games: &[FullGame]) -> Result<Vec<FullGame>> {
        self.games_repo
            .get_games()
            .context("failed to reload games from database")
    }

    fn handle_new_version(
        &self,
        game: &FullGame,
        monitoring_settings: MonitoringSettings,
    ) -> Result<Option<FullVersion>> {
        let cooldown_cutoff =
            Utc::now().naive_utc() - Duration::minutes(i64::from(monitoring_settings.version_cooldown_minutes));
        let latest_version_created_at = game.versions.iter().map(|version| version.created_at).max();

        if latest_version_created_at.is_some_and(|created_at| created_at > cooldown_cutoff) {
            return Ok(None);
        }

        println!("Creating new game version");

        let version = self
            .versions_repo
            .create_version(game.game.id)
            .context("failed to create new version")?;

        println!("Created new version with id: {}", version.version.id);

        Ok(Some(version))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Game, MonitoringSettings, Version};
    use crate::persistence::{MockGameRepository, MockVersionRepository};
    use chrono::{Duration, Utc};
    use notify_debouncer_full::notify::Event;
    use std::sync::mpsc;
    use tempfile::tempdir;

    #[test]
    fn test_handling_new_version_with_delay() {
        let mut games_repo = MockGameRepository::new();
        let mut versions_repo = MockVersionRepository::new();

        versions_repo
            .expect_create_version()
            .times(1)
            .returning(|game_id| {
                Ok(FullVersion {
                    version: Version {
                        id: 1,
                        game_id,
                        version_number: 2,
                        tag: None,
                        created_at: Utc::now().naive_utc(),
                        parent_id: Some(1),
                    },
                    game: Game {
                        id: game_id,
                        title: "Test Game".to_string(),
                        active_version_id: Some(2),
                        icon_path: None,
                    },
                })
            });

        games_repo.expect_get_games().returning(|| Ok(vec![]));

        let working_base = tempdir().unwrap();
        let source_dir = tempdir().unwrap();
        let source_path = source_dir.path().to_str().unwrap().to_string();

        let handler = FsEventHandler::new(
            working_base.path().to_path_buf(),
            Arc::new(games_repo),
            Arc::new(versions_repo),
        );

        let (notif_tx, notif_rx) = mpsc::channel();

        // Game last version is intentionally old enough to allow a new version.
        let game = FullGame {
            game: Game {
                id: 1,
                title: "Test Game".to_string(),
                active_version_id: Some(1),
                icon_path: None,
            },
            last_version: Some(Version {
                id: 0,
                game_id: 1,
                version_number: 1,
                tag: None,
                created_at: (Utc::now() - Duration::minutes(20)).naive_utc(),
                parent_id: None,
            }),
            tracked_dirs: vec![TrackedDir {
                id: 1,
                game_id: 1,
                path: source_path.clone(),
            }],
            versions: vec![Version {
                id: 0,
                game_id: 1,
                version_number: 1,
                tag: None,
                created_at: (Utc::now() - Duration::minutes(20)).naive_utc(),
                parent_id: None,
            }],
        };

        let mut event = Event::new(notify_debouncer_full::notify::EventKind::Any);
        event = event.add_path(source_dir.path().join("save.dat"));

        let events = vec![DebouncedEvent {
            event,
            time: std::time::Instant::now(),
        }];

        handler
            .run(
                events,
                notif_tx,
                &[game],
                working_base.path(),
                MonitoringSettings::default(),
            )
            .unwrap();

        let mut received_version = false;
        let mut received_change = false;

        while let Ok(msg) = notif_rx.try_recv() {
            match msg {
                Notification::VersionCreated {
                    game_id,
                    version_number,
                } => {
                    assert_eq!(game_id, 1);
                    assert_eq!(version_number, 2);
                    received_version = true;
                }
                Notification::ChangeDetected { game_id } => {
                    assert_eq!(game_id, 1);
                    received_change = true;
                }
                Notification::Error(_) => panic!("Should not receive error"),
            }
        }

        assert!(
            received_version,
            "Did not receive VersionCreated notification"
        );
        assert!(
            received_change,
            "Did not receive ChangeDetected notification"
        );
    }

    #[test]
    fn test_always_creates_version_on_debounce_fire() {
        // If the cooldown has elapsed, a debounced batch should create a version.
        let mut games_repo = MockGameRepository::new();
        let mut versions_repo = MockVersionRepository::new();

        versions_repo
            .expect_create_version()
            .times(1)
            .returning(|game_id| {
                Ok(FullVersion {
                    version: Version {
                        id: 2,
                        game_id,
                        version_number: 2,
                        tag: None,
                        created_at: Utc::now().naive_utc(),
                        parent_id: Some(1),
                    },
                    game: Game {
                        id: game_id,
                        title: "Test Game".to_string(),
                        active_version_id: Some(2),
                        icon_path: None,
                    },
                })
            });

        games_repo.expect_get_games().returning(|| Ok(vec![]));

        let working_base = tempdir().unwrap();
        let source_dir = tempdir().unwrap();
        let source_path = source_dir.path().to_str().unwrap().to_string();

        let handler = FsEventHandler::new(
            working_base.path().to_path_buf(),
            Arc::new(games_repo),
            Arc::new(versions_repo),
        );

        let (notif_tx, notif_rx) = mpsc::channel();

        let game = FullGame {
            game: Game {
                id: 1,
                title: "Test Game".to_string(),
                active_version_id: Some(1),
                icon_path: None,
            },
            last_version: Some(Version {
                id: 1,
                game_id: 1,
                version_number: 1,
                tag: None,
                created_at: (Utc::now() - Duration::minutes(16)).naive_utc(),
                parent_id: None,
            }),
            tracked_dirs: vec![TrackedDir {
                id: 1,
                game_id: 1,
                path: source_path.clone(),
            }],
            versions: vec![Version {
                id: 1,
                game_id: 1,
                version_number: 1,
                tag: None,
                created_at: (Utc::now() - Duration::minutes(16)).naive_utc(),
                parent_id: None,
            }],
        };

        let mut event = Event::new(notify_debouncer_full::notify::EventKind::Any);
        event = event.add_path(source_dir.path().join("save.dat"));

        let events = vec![DebouncedEvent {
            event,
            time: std::time::Instant::now(),
        }];

        handler
            .run(
                events,
                notif_tx,
                &[game],
                working_base.path(),
                MonitoringSettings::default(),
            )
            .unwrap();

        let mut received_version = false;
        let mut received_change = false;

        while let Ok(msg) = notif_rx.try_recv() {
            match msg {
                Notification::VersionCreated {
                    game_id,
                    version_number,
                } => {
                    assert_eq!(game_id, 1);
                    assert_eq!(version_number, 2);
                    received_version = true;
                }
                Notification::ChangeDetected { game_id } => {
                    assert_eq!(game_id, 1);
                    received_change = true;
                }
                Notification::Error(_) => panic!("Should not receive error"),
            }
        }

        assert!(
            received_version,
            "Did not receive VersionCreated notification"
        );
        assert!(
            received_change,
            "Did not receive ChangeDetected notification"
        );
    }

    #[test]
    fn test_multiple_events_in_batch_create_single_version() {
        // Multiple file events grouped into one debounce batch must produce exactly one version.
        let mut games_repo = MockGameRepository::new();
        let mut versions_repo = MockVersionRepository::new();

        versions_repo
            .expect_create_version()
            .times(1)
            .returning(|game_id| {
                Ok(FullVersion {
                    version: Version {
                        id: 2,
                        game_id,
                        version_number: 2,
                        tag: None,
                        created_at: Utc::now().naive_utc(),
                        parent_id: Some(1),
                    },
                    game: Game {
                        id: game_id,
                        title: "Test Game".to_string(),
                        active_version_id: Some(2),
                        icon_path: None,
                    },
                })
            });

        games_repo.expect_get_games().returning(|| Ok(vec![]));

        let working_base = tempdir().unwrap();
        let source_dir = tempdir().unwrap();
        let source_path = source_dir.path().to_str().unwrap().to_string();

        let handler = FsEventHandler::new(
            working_base.path().to_path_buf(),
            Arc::new(games_repo),
            Arc::new(versions_repo),
        );

        let (notif_tx, notif_rx) = mpsc::channel();

        let game = FullGame {
            game: Game {
                id: 1,
                title: "Test Game".to_string(),
                active_version_id: Some(1),
                icon_path: None,
            },
            last_version: None,
            tracked_dirs: vec![TrackedDir {
                id: 1,
                game_id: 1,
                path: source_path.clone(),
            }],
            versions: vec![],
        };

        let make_event = |filename: &str| {
            let mut e = Event::new(notify_debouncer_full::notify::EventKind::Any);
            e = e.add_path(source_dir.path().join(filename));
            DebouncedEvent {
                event: e,
                time: std::time::Instant::now(),
            }
        };

        let events = vec![
            make_event("save1.dat"),
            make_event("save2.dat"),
            make_event("save3.dat"),
        ];

        handler
            .run(
                events,
                notif_tx,
                &[game],
                working_base.path(),
                MonitoringSettings::default(),
            )
            .unwrap();

        let mut version_count = 0;
        let mut received_change = false;

        while let Ok(msg) = notif_rx.try_recv() {
            match msg {
                Notification::VersionCreated { .. } => version_count += 1,
                Notification::ChangeDetected { .. } => received_change = true,
                Notification::Error(_) => panic!("Should not receive error"),
            }
        }

        assert_eq!(
            version_count, 1,
            "Expected exactly 1 version for a batch of events"
        );
        assert!(received_change);
    }

    #[test]
    fn test_recent_version_suppresses_new_version_creation() {
        let games_repo = MockGameRepository::new();
        let mut versions_repo = MockVersionRepository::new();
        versions_repo.expect_create_version().times(0);

        let working_base = tempdir().unwrap();
        let source_dir = tempdir().unwrap();
        let source_path = source_dir.path().to_str().unwrap().to_string();

        let handler = FsEventHandler::new(
            working_base.path().to_path_buf(),
            Arc::new(games_repo),
            Arc::new(versions_repo),
        );

        let (notif_tx, notif_rx) = mpsc::channel();
        let created_at = Utc::now().naive_utc();

        let game = FullGame {
            game: Game {
                id: 1,
                title: "Test Game".to_string(),
                active_version_id: Some(1),
                icon_path: None,
            },
            last_version: Some(Version {
                id: 1,
                game_id: 1,
                version_number: 1,
                tag: None,
                created_at,
                parent_id: None,
            }),
            tracked_dirs: vec![TrackedDir {
                id: 1,
                game_id: 1,
                path: source_path.clone(),
            }],
            versions: vec![Version {
                id: 1,
                game_id: 1,
                version_number: 1,
                tag: None,
                created_at,
                parent_id: None,
            }],
        };

        let mut event = Event::new(notify_debouncer_full::notify::EventKind::Any);
        event = event.add_path(source_dir.path().join("save.dat"));

        let events = vec![DebouncedEvent {
            event,
            time: std::time::Instant::now(),
        }];

        handler
            .run(
                events,
                notif_tx,
                &[game],
                working_base.path(),
                MonitoringSettings::default(),
            )
            .unwrap();

        let mut received_version = false;
        let mut received_change = false;

        while let Ok(msg) = notif_rx.try_recv() {
            match msg {
                Notification::VersionCreated { .. } => received_version = true,
                Notification::ChangeDetected { game_id } => {
                    assert_eq!(game_id, 1);
                    received_change = true;
                }
                Notification::Error(_) => panic!("Should not receive error"),
            }
        }

        assert!(
            !received_version,
            "Should not create a version during the cooldown window"
        );
        assert!(received_change);
    }
}
