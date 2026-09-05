use crate::detection::{Notification, NotificationSender};
use crate::models::{FullGame, FullVersion, MonitoringSettings, TrackedDir};
use crate::persistence::{GameRepository, VersionRepository};
use crate::repository::VersionReason;
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
                let Some(version) = game.last_version.as_ref() else {
                    return false;
                };
                let Some(slot) = game.active_slot.as_ref() else {
                    return false;
                };

                let Ok(working_dir_path) = crate::repository::game_snapshot_dir(
                    working_base,
                    &game.game,
                    slot,
                    version,
                    dir,
                ) else {
                    return false;
                };

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

            // `need_rescan` is notify's signal that events were dropped, so the
            // event paths cannot be trusted and every tracked directory has to be
            // treated as modified. Fall through to the normal flow to do that: it
            // creates a version first and only then copies into it. This used to
            // call `handle_rescan`, which deletes and refills the stored snapshot
            // of `last_version` directly and then returned early — so a rescan
            // both rewrote an existing version's content (versions are immutable)
            // and never created a version to account for the change.
            if ev.need_rescan() {
                for dir in &dirs {
                    modified_dirs.insert(dir.id);
                    modified_games.insert(dir.game_id);
                }
                continue;
            }

            // Find which tracked directories were affected by this event
            for path in &ev.paths {
                if let Some(name) = path.file_name().and_then(|n| n.to_str())
                    && crate::fs_utils::is_excluded_file(name)
                {
                    continue;
                }

                for dir in &dirs {
                    if let Ok(relative) = path.strip_prefix(&dir.path) {
                        // A user-excluded file is never captured, so a change to
                        // it must not spawn a version either.
                        if crate::exclusions::is_excluded(
                            &crate::exclusions::parse(&dir.exclusions),
                            relative,
                        ) {
                            continue;
                        }
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

        // Now copy all modified tracked directories into the correct version.
        // Only games that actually got a version this pass may be synced. Versions
        // are immutable: syncing without a fresh one would delete and refill the
        // stored snapshot of `last_version`, replacing an existing version's content
        // with current live state. That is worst on the cooldown path, because
        // `last_version` is the slot's ACTIVE version while the cooldown compares
        // against max(created_at) — so after restoring an older version, a younger
        // sibling keeps the cooldown closed and the restored version's history gets
        // overwritten with post-restore live state, with no new entry to show for it.
        let dirs_to_sync: Vec<&TrackedDir> = dirs
            .iter()
            .filter(|dir| modified_dirs.contains(&dir.id) && new_versions.contains(&dir.game_id))
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
        let Some(version) = game.last_version.as_ref() else {
            return Ok(());
        };

        let source_path = Path::new(&dir.path);
        let working_dir_path = crate::repository::game_snapshot_dir(
            &self.working_base,
            &game.game,
            game.active_slot
                .as_ref()
                .context("Game is missing an active slot")?,
            version,
            dir,
        )?;

        // Remove the existing directory if it exists
        if working_dir_path.exists() {
            fs::remove_dir_all(&working_dir_path)?;
        }

        // Ensure parent directory exists
        if let Some(parent) = working_dir_path.parent()
            && !parent.exists()
        {
            fs::create_dir_all(parent)?;
        }

        crate::fs_utils::copy_dir_filtered(
            source_path,
            &working_dir_path,
            &crate::exclusions::parse(&dir.exclusions),
        )?;

        Ok(())
    }

    /// Fills the snapshot directories of `dirs` from their live state. Only the
    /// first-scan caller may use this: it is destructive, and `first_scan_dirs` is
    /// filtered to snapshots that do not exist yet.
    fn handle_rescan(&self, dirs: &[TrackedDir], games: &[FullGame]) -> Result<()> {
        for dir in dirs {
            let Some(game) = games.iter().find(|g| g.game.id == dir.game_id) else {
                continue;
            };
            let Some(version) = game.last_version.as_ref() else {
                continue;
            };

            let path = Path::new(&dir.path);
            let working_dir_path = crate::repository::game_snapshot_dir(
                &self.working_base,
                &game.game,
                game.active_slot
                    .as_ref()
                    .context("Game is missing an active slot")?,
                version,
                dir,
            )?;

            if working_dir_path.exists() {
                fs::remove_dir_all(&working_dir_path)?;
            } else if !working_dir_path.parent().unwrap().exists() {
                fs::create_dir_all(working_dir_path.parent().unwrap())?;
            }

            crate::fs_utils::copy_dir_filtered(
                path,
                &working_dir_path,
                &crate::exclusions::parse(&dir.exclusions),
            )?;
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
        let cooldown_cutoff = Utc::now().naive_utc()
            - Duration::minutes(i64::from(monitoring_settings.version_cooldown_minutes));
        let latest_version_created_at =
            game.versions.iter().map(|version| version.created_at).max();

        if latest_version_created_at.is_some_and(|created_at| created_at > cooldown_cutoff) {
            return Ok(None);
        }

        println!("Creating new game version");

        let version = self
            .versions_repo
            .create_version(game.game.id, VersionReason::Watcher)
            .context("failed to create new version")?;

        println!("Created new version with id: {}", version.version.id);

        Ok(Some(version))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Game, MonitoringSettings, Slot, Version};
    use crate::persistence::{MockGameRepository, MockVersionRepository};
    use chrono::{Duration, Utc};
    use notify_debouncer_full::notify::Event;
    use std::sync::mpsc;
    use tempfile::tempdir;

    fn default_slot(game_id: i32, active_version_id: Option<i32>) -> Slot {
        Slot {
            id: 10,
            uuid: "27b91a04-0000-0000-0000-000000000000".to_string(),
            game_id,
            name: "Default Slot".to_string(),
            name_normalized: "default slot".to_string(),
            folder_slug: "default-slot__s_27b91a04".to_string(),
            active_version_id,
            created_at: Utc::now().naive_utc(),
            updated_at: Utc::now().naive_utc(),
        }
    }

    #[test]
    fn test_handling_new_version_with_delay() {
        let mut games_repo = MockGameRepository::new();
        let mut versions_repo = MockVersionRepository::new();

        versions_repo
            .expect_create_version()
            .times(1)
            .returning(|game_id, _reason| {
                Ok(FullVersion {
                    version: Version {
                        id: 1,
                        uuid: None,
                        folder_slug: None,
                        slot_id: 10,
                        version_number: 2,
                        tag: None,
                        reason: "watcher".to_string(),
                        created_at: Utc::now().naive_utc(),
                        parent_id: Some(1),
                    },
                    game: Game {
                        id: game_id,
                        uuid: None,
                        title: "Test Game".to_string(),
                        title_normalized: None,
                        folder_slug: None,
                        active_slot_id: Some(10),
                        icon_path: None,
                    },
                    slot: default_slot(game_id, Some(2)),
                    parent_uuid: None,
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
                uuid: None,
                title: "Test Game".to_string(),
                title_normalized: None,
                folder_slug: None,
                active_slot_id: Some(10),
                icon_path: None,
            },
            slots: vec![default_slot(1, Some(1))],
            active_slot: Some(default_slot(1, Some(1))),
            last_version: Some(Version {
                id: 0,
                uuid: None,
                folder_slug: None,
                slot_id: 10,
                version_number: 1,
                tag: None,
                reason: "watcher".to_string(),
                created_at: (Utc::now() - Duration::minutes(20)).naive_utc(),
                parent_id: None,
            }),
            tracked_dirs: vec![TrackedDir {
                id: 1,
                uuid: None,
                folder_slug: None,
                game_id: 1,
                path: source_path.clone(),
                exclusions: "[]".to_string(),
            }],
            versions: vec![Version {
                id: 0,
                uuid: None,
                folder_slug: None,
                slot_id: 10,
                version_number: 1,
                tag: None,
                reason: "watcher".to_string(),
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
            .returning(|game_id, _reason| {
                Ok(FullVersion {
                    version: Version {
                        id: 2,
                        uuid: None,
                        folder_slug: None,
                        slot_id: 10,
                        version_number: 2,
                        tag: None,
                        reason: "watcher".to_string(),
                        created_at: Utc::now().naive_utc(),
                        parent_id: Some(1),
                    },
                    game: Game {
                        id: game_id,
                        uuid: None,
                        title: "Test Game".to_string(),
                        title_normalized: None,
                        folder_slug: None,
                        active_slot_id: Some(10),
                        icon_path: None,
                    },
                    slot: default_slot(game_id, Some(2)),
                    parent_uuid: None,
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
                uuid: None,
                title: "Test Game".to_string(),
                title_normalized: None,
                folder_slug: None,
                active_slot_id: Some(10),
                icon_path: None,
            },
            slots: vec![default_slot(1, Some(1))],
            active_slot: Some(default_slot(1, Some(1))),
            last_version: Some(Version {
                id: 1,
                uuid: None,
                folder_slug: None,
                slot_id: 10,
                version_number: 1,
                tag: None,
                reason: "watcher".to_string(),
                created_at: (Utc::now() - Duration::minutes(16)).naive_utc(),
                parent_id: None,
            }),
            tracked_dirs: vec![TrackedDir {
                id: 1,
                uuid: None,
                folder_slug: None,
                game_id: 1,
                path: source_path.clone(),
                exclusions: "[]".to_string(),
            }],
            versions: vec![Version {
                id: 1,
                uuid: None,
                folder_slug: None,
                slot_id: 10,
                version_number: 1,
                tag: None,
                reason: "watcher".to_string(),
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
            .returning(|game_id, _reason| {
                Ok(FullVersion {
                    version: Version {
                        id: 2,
                        uuid: None,
                        folder_slug: None,
                        slot_id: 10,
                        version_number: 2,
                        tag: None,
                        reason: "watcher".to_string(),
                        created_at: Utc::now().naive_utc(),
                        parent_id: Some(1),
                    },
                    game: Game {
                        id: game_id,
                        uuid: None,
                        title: "Test Game".to_string(),
                        title_normalized: None,
                        folder_slug: None,
                        active_slot_id: Some(10),
                        icon_path: None,
                    },
                    slot: default_slot(game_id, Some(2)),
                    parent_uuid: None,
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
                uuid: None,
                title: "Test Game".to_string(),
                title_normalized: None,
                folder_slug: None,
                active_slot_id: Some(10),
                icon_path: None,
            },
            slots: vec![default_slot(1, None)],
            active_slot: Some(default_slot(1, None)),
            last_version: None,
            tracked_dirs: vec![TrackedDir {
                id: 1,
                uuid: None,
                folder_slug: None,
                game_id: 1,
                path: source_path.clone(),
                exclusions: "[]".to_string(),
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
    fn test_rescan_uses_last_version_number_not_active_version_id() {
        let mut games_repo = MockGameRepository::new();
        let versions_repo = MockVersionRepository::new();

        games_repo.expect_get_games().never();

        let working_base = tempdir().unwrap();
        let source_dir = tempdir().unwrap();
        let source_path = source_dir.path().to_str().unwrap().to_string();
        fs::write(source_dir.path().join("save.dat"), "hello").unwrap();

        let handler = FsEventHandler::new(
            working_base.path().to_path_buf(),
            Arc::new(games_repo),
            Arc::new(versions_repo),
        );

        let (notif_tx, _notif_rx) = mpsc::channel();

        let game = FullGame {
            game: Game {
                id: 1,
                uuid: None,
                title: "Test Game".to_string(),
                title_normalized: None,
                folder_slug: Some("test-game__g_deadbeef".to_string()),
                active_slot_id: Some(10),
                icon_path: None,
            },
            slots: vec![default_slot(1, Some(99))],
            active_slot: Some(default_slot(1, Some(99))),
            last_version: Some(Version {
                id: 99,
                uuid: None,
                folder_slug: Some("v003__v_abcdef12".to_string()),
                slot_id: 10,
                version_number: 3,
                tag: None,
                reason: "watcher".to_string(),
                created_at: Utc::now().naive_utc(),
                parent_id: None,
            }),
            tracked_dirs: vec![TrackedDir {
                id: 1,
                uuid: None,
                folder_slug: Some("saves__d_12345678".to_string()),
                game_id: 1,
                path: source_path,
                exclusions: "[]".to_string(),
            }],
            versions: vec![],
        };

        handler
            .run(
                vec![],
                notif_tx,
                &[game.clone()],
                working_base.path(),
                MonitoringSettings::default(),
            )
            .unwrap();

        let expected_path = crate::repository::game_snapshot_dir(
            working_base.path(),
            &game.game,
            game.active_slot.as_ref().unwrap(),
            game.last_version.as_ref().unwrap(),
            &game.tracked_dirs[0],
        )
        .unwrap()
        .join("save.dat");
        let wrong_path = working_base
            .path()
            .join("1")
            .join("99")
            .join("1")
            .join("save.dat");

        assert!(
            expected_path.exists(),
            "expected snapshot path should exist"
        );
        assert!(
            !wrong_path.exists(),
            "active_version_id should not be used as folder name"
        );
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
                uuid: None,
                title: "Test Game".to_string(),
                title_normalized: None,
                folder_slug: Some("test-game__g_deadbeef".to_string()),
                active_slot_id: Some(10),
                icon_path: None,
            },
            slots: vec![default_slot(1, Some(1))],
            active_slot: Some(default_slot(1, Some(1))),
            last_version: Some(Version {
                id: 1,
                uuid: None,
                folder_slug: Some("v001__v_abcdef12".to_string()),
                slot_id: 10,
                version_number: 1,
                tag: None,
                reason: "watcher".to_string(),
                created_at,
                parent_id: None,
            }),
            tracked_dirs: vec![TrackedDir {
                id: 1,
                uuid: None,
                folder_slug: Some("saves__d_12345678".to_string()),
                game_id: 1,
                path: source_path.clone(),
                exclusions: "[]".to_string(),
            }],
            versions: vec![Version {
                id: 1,
                uuid: None,
                folder_slug: Some("v001__v_abcdef12".to_string()),
                slot_id: 10,
                version_number: 1,
                tag: None,
                reason: "watcher".to_string(),
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

    /// Builds a game whose single tracked dir lives at `source_path` and whose
    /// active version is `version`, with slugs set so `game_snapshot_dir` resolves.
    fn rescan_game(source_path: String, version: Version) -> FullGame {
        FullGame {
            game: Game {
                id: 1,
                uuid: None,
                title: "Test Game".to_string(),
                title_normalized: None,
                folder_slug: Some("test-game__g_deadbeef".to_string()),
                active_slot_id: Some(10),
                icon_path: None,
            },
            slots: vec![default_slot(1, Some(version.id))],
            active_slot: Some(default_slot(1, Some(version.id))),
            last_version: Some(version.clone()),
            tracked_dirs: vec![TrackedDir {
                id: 1,
                uuid: None,
                folder_slug: Some("saves__d_12345678".to_string()),
                game_id: 1,
                path: source_path,
                exclusions: "[]".to_string(),
            }],
            versions: vec![version],
        }
    }

    fn aged_version(id: i32, number: i32, slug: &str, age_minutes: i64) -> Version {
        Version {
            id,
            uuid: None,
            folder_slug: Some(slug.to_string()),
            slot_id: 10,
            version_number: number,
            tag: None,
            reason: "watcher".to_string(),
            created_at: (Utc::now() - Duration::minutes(age_minutes)).naive_utc(),
            parent_id: None,
        }
    }

    fn rescan_event() -> DebouncedEvent {
        DebouncedEvent {
            event: Event::new(notify_debouncer_full::notify::EventKind::Any)
                .set_flag(notify_debouncer_full::notify::event::Flag::Rescan),
            time: std::time::Instant::now(),
        }
    }

    #[test]
    fn test_rescan_must_not_overwrite_an_existing_versions_snapshot() {
        // Regression (Critical, pre-existing): on `need_rescan` the handler called
        // `handle_rescan` for EVERY tracked dir and returned early. That deletes
        // `last_version`'s stored snapshot and refills it from live state, so an
        // already-created version silently acquires new content, with no new entry
        // to account for it. Versions are immutable. A version-count assertion
        // cannot see this, so this asserts on the snapshot's bytes.
        let games_repo = MockGameRepository::new();
        let mut versions_repo = MockVersionRepository::new();
        // The cooldown is closed, so nothing may be created either.
        versions_repo.expect_create_version().times(0);

        let working_base = tempdir().unwrap();
        let source_dir = tempdir().unwrap();
        fs::write(source_dir.path().join("save.dat"), "live-state").unwrap();

        let handler = FsEventHandler::new(
            working_base.path().to_path_buf(),
            Arc::new(games_repo),
            Arc::new(versions_repo),
        );

        // V1 is seconds old, so the cooldown is closed.
        let v1 = aged_version(1, 1, "v001__v_abcdef12", 0);
        let game = rescan_game(source_dir.path().to_str().unwrap().to_string(), v1);

        // V1 already has a snapshot, holding content distinct from live state.
        let v1_snapshot = crate::repository::game_snapshot_dir(
            working_base.path(),
            &game.game,
            game.active_slot.as_ref().unwrap(),
            game.last_version.as_ref().unwrap(),
            &game.tracked_dirs[0],
        )
        .unwrap();
        fs::create_dir_all(&v1_snapshot).unwrap();
        fs::write(v1_snapshot.join("save.dat"), "v1-content").unwrap();

        let (notif_tx, notif_rx) = mpsc::channel();
        handler
            .run(
                vec![rescan_event()],
                notif_tx,
                &[game],
                working_base.path(),
                MonitoringSettings::default(),
            )
            .unwrap();

        assert_eq!(
            fs::read_to_string(v1_snapshot.join("save.dat")).unwrap(),
            "v1-content",
            "a rescan must leave an existing version's stored snapshot byte-identical; \
             versions are immutable"
        );
        assert!(
            notif_rx
                .try_iter()
                .any(|n| matches!(n, Notification::ChangeDetected { game_id: 1 })),
            "a rescan must still report that the game changed"
        );
    }

    #[test]
    fn test_rescan_captures_live_state_into_a_fresh_version() {
        // The other half: a rescan exists to recover after notify dropped events,
        // so when the cooldown allows, it must still create a version and capture
        // the live state into THAT version — never into the previous one.
        let mut games_repo = MockGameRepository::new();
        let mut versions_repo = MockVersionRepository::new();

        let working_base = tempdir().unwrap();
        let source_dir = tempdir().unwrap();
        fs::write(source_dir.path().join("save.dat"), "live-state").unwrap();
        let source_path = source_dir.path().to_str().unwrap().to_string();

        let v1 = aged_version(1, 1, "v001__v_abcdef12", 20);
        let v2 = aged_version(2, 2, "v002__v_beefcafe", 0);

        let v2_for_create = v2.clone();
        versions_repo
            .expect_create_version()
            .times(1)
            .returning(move |game_id, _reason| {
                Ok(FullVersion {
                    version: v2_for_create.clone(),
                    game: Game {
                        id: game_id,
                        uuid: None,
                        title: "Test Game".to_string(),
                        title_normalized: None,
                        folder_slug: Some("test-game__g_deadbeef".to_string()),
                        active_slot_id: Some(10),
                        icon_path: None,
                    },
                    slot: default_slot(game_id, Some(2)),
                    parent_uuid: None,
                })
            });

        // After creation the handler reloads its games: V2 is now the active version.
        let reloaded_path = source_path.clone();
        let reloaded_v2 = v2.clone();
        games_repo.expect_get_games().returning(move || {
            Ok(vec![rescan_game(
                reloaded_path.clone(),
                reloaded_v2.clone(),
            )])
        });

        let handler = FsEventHandler::new(
            working_base.path().to_path_buf(),
            Arc::new(games_repo),
            Arc::new(versions_repo),
        );

        let game = rescan_game(source_path, v1);
        let snapshot_of = |v: &Version| {
            crate::repository::game_snapshot_dir(
                working_base.path(),
                &game.game,
                game.active_slot.as_ref().unwrap(),
                v,
                &game.tracked_dirs[0],
            )
            .unwrap()
        };
        let v1_snapshot = snapshot_of(game.last_version.as_ref().unwrap());
        fs::create_dir_all(&v1_snapshot).unwrap();
        fs::write(v1_snapshot.join("save.dat"), "v1-content").unwrap();
        let v2_snapshot = snapshot_of(&v2);

        let (notif_tx, _notif_rx) = mpsc::channel();
        handler
            .run(
                vec![rescan_event()],
                notif_tx,
                &[game.clone()],
                working_base.path(),
                MonitoringSettings::default(),
            )
            .unwrap();

        assert_eq!(
            fs::read_to_string(v2_snapshot.join("save.dat")).unwrap(),
            "live-state",
            "the rescan must capture the live state into the version it just created"
        );
        assert_eq!(
            fs::read_to_string(v1_snapshot.join("save.dat")).unwrap(),
            "v1-content",
            "the previous version's snapshot must be untouched"
        );
    }
}
