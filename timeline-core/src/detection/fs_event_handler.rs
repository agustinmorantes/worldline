use crate::checkpoint::CheckpointOutcome;
use crate::detection::{Notification, NotificationSender};
use crate::models::{FullGame, MonitoringSettings, TrackedDir};
use crate::persistence::{GameRepository, VersionRepository};
use crate::repository::VersionReason;
use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use notify_debouncer_full::DebouncedEvent;
use std::collections::HashSet;
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

        // A version row without a committed manifest needs one authoritative
        // capture before it can be treated as the active baseline.
        let mut modified_games: HashSet<i32> = HashSet::new();
        for game in &games {
            let Some(version) = game.last_version.as_ref() else {
                continue;
            };
            let Some(slot) = game.active_slot.as_ref() else {
                continue;
            };
            let Some(game_slug) = game.game.folder_slug.as_deref() else {
                continue;
            };
            let Some(version_slug) = version.folder_slug.as_deref() else {
                continue;
            };
            let version_dir = crate::repository::version_dir(
                &crate::repository::slot_dir(
                    &crate::repository::game_dir(working_base, game_slug),
                    &slot.folder_slug,
                ),
                version_slug,
            );
            if !crate::repository::metadata_path(&version_dir).exists() {
                modified_games.insert(game.game.id);
            }
        }

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
                        modified_games.insert(dir.game_id);
                    }
                }
            }
        }

        // Capture every tracked directory before deciding whether a new row is
        // needed. The manifest, not the event path, is authoritative.
        let mut created_games: HashSet<i32> = HashSet::new();
        for game_id in &modified_games {
            let game = games
                .iter()
                .find(|g| g.game.id == *game_id)
                .context("failed to find modified game")?;

            if let Some(CheckpointOutcome::Created(version)) = self
                .handle_new_version(game, monitoring_settings)
                .context("failed to handle new version")?
            {
                created_games.insert(*game_id);

                // Send notification about new version
                notif_tx.send(Notification::VersionCreated {
                    game_id: *game_id,
                    version_number: version.version.version_number,
                })?;
            }
        }

        // Reload games if new versions were created to get updated active_version_id
        let games = if !created_games.is_empty() {
            self.reload_games(&games)?
        } else {
            games
        };

        // Send change detected notifications
        for game_id in &modified_games {
            notif_tx.send(Notification::ChangeDetected { game_id: *game_id })?;
        }

        Ok(games)
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
    ) -> Result<Option<CheckpointOutcome>> {
        let cooldown_cutoff = Utc::now().naive_utc()
            - Duration::minutes(i64::from(monitoring_settings.version_cooldown_minutes));
        let latest_version_created_at =
            game.versions.iter().map(|version| version.created_at).max();

        if latest_version_created_at.is_some_and(|created_at| created_at > cooldown_cutoff) {
            return Ok(None);
        }

        println!("Creating new game version");

        let outcome = crate::checkpoint::checkpoint_current_state(
            &self.working_base,
            self.versions_repo.as_ref(),
            game,
            VersionReason::Watcher,
        )?;

        if let CheckpointOutcome::Created(version) = &outcome {
            println!("Created new version with id: {}", version.version.id);
        }

        Ok(Some(outcome))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{FullVersion, Game, MonitoringSettings, Slot, Version};
    use crate::persistence::{MockGameRepository, MockVersionRepository};
    use chrono::{Duration, Utc};
    use notify_debouncer_full::notify::Event;
    use std::fs;
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

    fn with_test_tracked_dir_uuids(mut game: FullGame) -> FullGame {
        for (index, dir) in game.tracked_dirs.iter_mut().enumerate() {
            dir.uuid = Some(uuid::Uuid::from_u128((index + 1) as u128).to_string());
        }
        game
    }

    #[test]
    fn same_bytes_event_does_not_create_version_and_changed_event_captures_all_dirs() {
        let working_base = tempdir().unwrap();
        let first_dir = tempdir().unwrap();
        let second_dir = tempdir().unwrap();
        fs::write(first_dir.path().join("first.dat"), b"first").unwrap();
        fs::write(second_dir.path().join("second.dat"), b"second").unwrap();

        let first_uuid = uuid::Uuid::from_u128(1);
        let second_uuid = uuid::Uuid::from_u128(2);
        let tracked_dirs = vec![
            TrackedDir {
                id: 1,
                uuid: Some(first_uuid.to_string()),
                folder_slug: Some("first__d_00000001".to_string()),
                game_id: 1,
                path: first_dir.path().to_string_lossy().into_owned(),
                exclusions: "[]".to_string(),
            },
            TrackedDir {
                id: 2,
                uuid: Some(second_uuid.to_string()),
                folder_slug: Some("second__d_00000002".to_string()),
                game_id: 1,
                path: second_dir.path().to_string_lossy().into_owned(),
                exclusions: "[]".to_string(),
            },
        ];
        let game_model = Game {
            id: 1,
            uuid: Some("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa".to_string()),
            title: "Test Game".to_string(),
            title_normalized: Some("test game".to_string()),
            folder_slug: Some("test-game__g_aaaaaaaa".to_string()),
            active_slot_id: Some(10),
            icon_path: None,
        };
        let slot = default_slot(1, Some(1));
        let version = Version {
            id: 1,
            uuid: Some("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb".to_string()),
            folder_slug: Some("v001__v_bbbbbbbb".to_string()),
            slot_id: 10,
            version_number: 1,
            tag: None,
            reason: "watcher".to_string(),
            created_at: (Utc::now() - Duration::minutes(20)).naive_utc(),
            parent_id: None,
        };
        let full_version = FullVersion {
            version: version.clone(),
            game: game_model.clone(),
            slot: slot.clone(),
            parent_uuid: None,
        };
        let game = with_test_tracked_dir_uuids(FullGame {
            game: game_model.clone(),
            slots: vec![slot.clone()],
            active_slot: Some(slot),
            last_version: Some(version.clone()),
            tracked_dirs: tracked_dirs.clone(),
            versions: vec![version.clone()],
        });

        let snapshot =
            crate::content_store::capture_snapshot(working_base.path(), &tracked_dirs).unwrap();
        let version_entity_dir = crate::repository::version_dir(
            &crate::repository::slot_dir(
                &crate::repository::game_dir(
                    working_base.path(),
                    game_model.folder_slug.as_deref().unwrap(),
                ),
                &full_version.slot.folder_slug,
            ),
            full_version.version.folder_slug.as_deref().unwrap(),
        );
        crate::repository::write_metadata_json(
            &version_entity_dir,
            &crate::repository::VersionMetadata {
                entity: "version".to_string(),
                uuid: uuid::Uuid::parse_str(full_version.version.uuid.as_deref().unwrap()).unwrap(),
                game_uuid: uuid::Uuid::parse_str(full_version.game.uuid.as_deref().unwrap())
                    .unwrap(),
                slot_uuid: Some(uuid::Uuid::parse_str(&full_version.slot.uuid).unwrap()),
                version_number: full_version.version.version_number,
                folder_slug: full_version.version.folder_slug.clone().unwrap(),
                parent_version_uuid: None,
                tag: None,
                reason: VersionReason::Watcher,
                created_at: chrono::DateTime::<Utc>::from_naive_utc_and_offset(
                    full_version.version.created_at,
                    Utc,
                ),
                content_hash: snapshot.content_hash,
                files: snapshot.files,
            },
        )
        .unwrap();

        let mut games_repo = MockGameRepository::new();
        let mut versions_repo = MockVersionRepository::new();
        let refreshed_game = FullGame {
            last_version: Some(Version {
                id: 2,
                uuid: Some("cccccccc-cccc-cccc-cccc-cccccccccccc".to_string()),
                folder_slug: Some("v002__v_cccccccc".to_string()),
                slot_id: 10,
                version_number: 2,
                tag: None,
                reason: "watcher".to_string(),
                created_at: Utc::now().naive_utc(),
                parent_id: Some(1),
            }),
            versions: vec![Version {
                id: 2,
                uuid: Some("cccccccc-cccc-cccc-cccc-cccccccccccc".to_string()),
                folder_slug: Some("v002__v_cccccccc".to_string()),
                slot_id: 10,
                version_number: 2,
                tag: None,
                reason: "watcher".to_string(),
                created_at: Utc::now().naive_utc(),
                parent_id: Some(1),
            }],
            ..game.clone()
        };
        games_repo
            .expect_get_games()
            .times(1)
            .returning(move || Ok(vec![refreshed_game.clone()]));
        let full_version_for_reload = full_version.clone();
        let parent_uuid = full_version.version.uuid.clone();
        versions_repo
            .expect_get_version()
            .times(1)
            .returning(move |_| Ok(full_version_for_reload.clone()));
        versions_repo
            .expect_create_version_from_snapshot()
            .times(1)
            .returning(move |_, _, snapshot| {
                assert!(
                    snapshot
                        .files
                        .iter()
                        .any(|file| file.tracked_dir_uuid == first_uuid)
                );
                assert!(
                    snapshot
                        .files
                        .iter()
                        .any(|file| file.tracked_dir_uuid == second_uuid)
                );
                Ok(FullVersion {
                    version: Version {
                        id: 2,
                        uuid: Some("cccccccc-cccc-cccc-cccc-cccccccccccc".to_string()),
                        folder_slug: Some("v002__v_cccccccc".to_string()),
                        slot_id: 10,
                        version_number: 2,
                        tag: None,
                        reason: "watcher".to_string(),
                        created_at: Utc::now().naive_utc(),
                        parent_id: Some(1),
                    },
                    game: game_model.clone(),
                    slot: default_slot(1, Some(2)),
                    parent_uuid: parent_uuid.clone(),
                })
            });

        let handler = FsEventHandler::new(
            working_base.path().to_path_buf(),
            Arc::new(games_repo),
            Arc::new(versions_repo),
        );
        let event = |path: &Path| DebouncedEvent {
            event: Event::new(notify_debouncer_full::notify::EventKind::Any)
                .add_path(path.to_path_buf()),
            time: std::time::Instant::now(),
        };

        let (notif_tx, notif_rx) = mpsc::channel();
        handler
            .run(
                vec![event(&first_dir.path().join("first.dat"))],
                notif_tx.clone(),
                std::slice::from_ref(&game),
                working_base.path(),
                MonitoringSettings::default(),
            )
            .unwrap();
        assert!(
            !notif_rx
                .try_iter()
                .any(|notification| matches!(notification, Notification::VersionCreated { .. }))
        );

        fs::write(first_dir.path().join("first.dat"), b"changed").unwrap();
        handler
            .run(
                vec![event(&first_dir.path().join("first.dat"))],
                notif_tx,
                std::slice::from_ref(&game),
                working_base.path(),
                MonitoringSettings::default(),
            )
            .unwrap();
        assert!(
            notif_rx
                .try_iter()
                .any(|notification| matches!(notification, Notification::VersionCreated { .. }))
        );
    }

    #[test]
    fn test_handling_new_version_with_delay() {
        let mut games_repo = MockGameRepository::new();
        let mut versions_repo = MockVersionRepository::new();

        versions_repo
            .expect_create_version_from_snapshot()
            .times(1)
            .returning(|game_id, _reason, _snapshot| {
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
        let game = with_test_tracked_dir_uuids(FullGame {
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
        });

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
            .expect_create_version_from_snapshot()
            .times(1)
            .returning(|game_id, _reason, _snapshot| {
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

        let game = with_test_tracked_dir_uuids(FullGame {
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
        });

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
            .expect_create_version_from_snapshot()
            .times(1)
            .returning(|game_id, _reason, _snapshot| {
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

        let game = with_test_tracked_dir_uuids(FullGame {
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
        });

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
        versions_repo.expect_create_version_from_snapshot().times(0);

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

        let game = with_test_tracked_dir_uuids(FullGame {
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
        });

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
