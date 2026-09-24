use diesel::prelude::*;
use diesel::r2d2::{ConnectionManager, Pool};
use diesel::sqlite::SqliteConnection;
use diesel_migrations::MigrationHarness;
use savegame_timeline_core::checkpoint::{self, CheckpointOutcome};
use savegame_timeline_core::models::{FullVersion, GameForm};
use savegame_timeline_core::persistence::sqlite::{
    SqliteGameRepository, SqliteScanRootsRepository, SqliteVersionRepository,
};
use savegame_timeline_core::persistence::{GameRepository, VersionRepository};
use savegame_timeline_core::repository::{
    self, TrackedDirMetadata, VersionMetadata, VersionReason,
};
use savegame_timeline_core::{MIGRATIONS, TimelineCore};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::{TempDir, tempdir};
use uuid::Uuid;

type TestPool = Pool<ConnectionManager<SqliteConnection>>;

struct App {
    core: TimelineCore,
    games: Arc<SqliteGameRepository>,
    versions: Arc<SqliteVersionRepository>,
    pool: TestPool,
    repository: PathBuf,
    _data: TempDir,
    _icons: TempDir,
    _notifications: savegame_timeline_core::detection::NotificationReceiver,
}

impl App {
    fn new() -> Self {
        let manager = ConnectionManager::<SqliteConnection>::new(":memory:");
        let pool = Pool::builder().max_size(1).build(manager).unwrap();
        pool.get()
            .unwrap()
            .run_pending_migrations(MIGRATIONS)
            .unwrap();
        let data = tempdir().unwrap();
        let repository = data.path().join("repository");
        let icons = tempdir().unwrap();
        let games = Arc::new(SqliteGameRepository::with_repository_root(
            pool.clone(),
            repository.clone(),
        ));
        let versions = Arc::new(SqliteVersionRepository::with_repository_root(
            pool.clone(),
            repository.clone(),
        ));
        let scan_roots = Arc::new(SqliteScanRootsRepository::new(pool.clone()));
        let (core, notifications) = TimelineCore::new(
            games.clone(),
            versions.clone(),
            scan_roots,
            repository.clone(),
            icons.path().to_path_buf(),
            Some(30.0),
        )
        .unwrap();
        Self {
            core,
            games,
            versions,
            pool,
            repository,
            _data: data,
            _icons: icons,
            _notifications: notifications,
        }
    }

    fn add_game(&self, title: &str, dirs: &[&TempDir]) -> i32 {
        let paths = dirs
            .iter()
            .map(|dir| dir.path().to_str().unwrap())
            .collect::<Vec<_>>();
        let game = self
            .core
            .save_game(
                GameForm {
                    id: None,
                    uuid: None,
                    title,
                    title_normalized: None,
                    folder_slug: None,
                    active_slot_id: None,
                    icon_path: None,
                },
                Some(paths),
            )
            .unwrap();
        game.id
    }

    fn checkpoint(&self, game_id: i32, reason: VersionReason) -> FullVersion {
        let game = self.core.get_game(game_id).unwrap();
        match checkpoint::checkpoint_current_state(
            &self.repository,
            self.versions.as_ref(),
            &game,
            reason,
        )
        .unwrap()
        {
            CheckpointOutcome::Created(version) => version,
            CheckpointOutcome::Unchanged(_) => panic!("fixture checkpoint unexpectedly unchanged"),
        }
    }

    fn version_metadata(&self, version: &FullVersion) -> VersionMetadata {
        let game_dir = repository::game_dir(
            &self.repository,
            version.game.folder_slug.as_deref().unwrap(),
        );
        let slot_dir = repository::slot_dir(&game_dir, &version.slot.folder_slug);
        let version_dir =
            repository::version_dir(&slot_dir, version.version.folder_slug.as_deref().unwrap());
        repository::read_metadata_json(&version_dir).unwrap()
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.core.shutdown();
    }
}

fn complete_source() -> (App, Vec<TempDir>, i32, Vec<FullVersion>) {
    let app = App::new();
    let first = tempdir().unwrap();
    let second = tempdir().unwrap();
    fs::write(first.path().join("save.dat"), b"shared-base").unwrap();
    fs::write(second.path().join("save.dat"), b"second-base").unwrap();
    let game_id = app.add_game("History Fixture", &[&first, &second]);

    let v1 = app.checkpoint(game_id, VersionReason::Initial);
    fs::write(first.path().join("save.dat"), b"retired-one").unwrap();
    fs::write(second.path().join("save.dat"), b"retired-two").unwrap();
    let v2 = app.checkpoint(game_id, VersionReason::Manual);
    app.core.restore_version(v1.version.id).unwrap();
    fs::write(second.path().join("save.dat"), b"branch-two").unwrap();
    let v3 = app.checkpoint(game_id, VersionReason::Manual);
    app.core
        .tag_version(v3.version.id, "before-fork".into())
        .unwrap();
    app.core.create_slot(game_id, "Alternate").unwrap();
    annotate_tracking_dirs(&app, game_id);

    let v2_metadata = app.version_metadata(&v2);
    let retired = savegame_timeline_core::pruning::RetirementMarker {
        version_uuid: Uuid::parse_str(v2.version.uuid.as_deref().unwrap()).unwrap(),
        slot_uuid: Uuid::parse_str(&v2.slot.uuid).unwrap(),
        content_hash: v2_metadata.content_hash,
        retired_at: chrono::Utc::now(),
    };
    savegame_timeline_core::pruning::write_marker(&app.repository, &retired).unwrap();
    (app, vec![first, second], game_id, vec![v1, v2, v3])
}

fn annotate_tracking_dirs(app: &App, game_id: i32) {
    let full = app.core.get_game(game_id).unwrap();
    for (index, tracked) in full.tracked_dirs.iter().enumerate() {
        let uuid = Uuid::parse_str(tracked.uuid.as_deref().unwrap()).unwrap();
        let slug = tracked.folder_slug.as_deref().unwrap();
        let game_dir =
            repository::game_dir(&app.repository, full.game.folder_slug.as_deref().unwrap());
        let tracked_dir = repository::tracked_dir_dir(&game_dir, slug);
        let mut metadata: TrackedDirMetadata =
            repository::read_metadata_json(&tracked_dir).unwrap();
        metadata.label = Some(format!("Saves {}", index + 1));
        metadata.exclusions = vec!["ignored.tmp".into()];
        repository::write_metadata_json(&tracked_dir, &metadata).unwrap();
        let mut connection = app.pool.get().unwrap();
        use savegame_timeline_core::schema::tracked_dirs::dsl as tracked_dsl;
        diesel::update(tracked_dsl::tracked_dirs.filter(tracked_dsl::uuid.eq(uuid.to_string())))
            .set((
                tracked_dsl::label.eq(metadata.label.clone()),
                tracked_dsl::exclusions.eq(serde_json::to_string(&metadata.exclusions).unwrap()),
            ))
            .execute(&mut connection)
            .unwrap();
    }
}

fn snapshot_tree(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn visit(root: &Path, current: &Path, output: &mut Vec<(PathBuf, Vec<u8>)>) {
        let mut entries = fs::read_dir(current)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        entries.sort();
        for path in entries {
            let relative = path.strip_prefix(root).unwrap().to_path_buf();
            if relative == Path::new(".repository.lock") {
                continue;
            }
            if path.is_dir() {
                output.push((relative, b"<directory>".to_vec()));
                visit(root, &path, output);
            } else {
                output.push((relative, fs::read(path).unwrap()));
            }
        }
    }
    let mut output = Vec::new();
    visit(root, root, &mut output);
    output
}

#[test]
fn round_trips_complete_history_without_touching_saves_and_is_idempotent() {
    let (source, _source_dirs, game_id, source_versions) = complete_source();
    let bundle_parent = tempdir().unwrap();
    let bundle = bundle_parent.path().join("portable-history");
    let before = snapshot_tree(&source.repository);
    let source_game = source.core.get_game(game_id).unwrap();
    assert!(source_game.game.active_slot_id.is_some());
    let exported = source.core.export_game_history(game_id, &bundle).unwrap();
    assert_eq!(snapshot_tree(&source.repository), before);
    assert_eq!(exported.slot_count, 2);
    assert_eq!(exported.retired_version_count, 1);
    assert_eq!(exported.version_count, 4);
    assert!(!bundle.join("devices").exists());
    let bundle_game_dir = fs::read_dir(bundle.join("games"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let game_metadata: serde_json::Value =
        repository::read_metadata_json(&bundle_game_dir).unwrap();
    assert!(game_metadata.get("active_slot_uuid").is_none());
    for slot in fs::read_dir(repository::slots_dir(&bundle_game_dir)).unwrap() {
        let slot_metadata: serde_json::Value =
            repository::read_metadata_json(&slot.unwrap().path()).unwrap();
        assert!(slot_metadata.get("active_version_uuid").is_none());
    }

    let summary = TimelineCore::inspect_history_bundle(&bundle).unwrap();
    assert_eq!(summary.game_uuid, exported.game_uuid);
    assert_eq!(summary.active_tracked_dirs.len(), 2);
    assert!(
        summary
            .active_tracked_dirs
            .iter()
            .all(|tracked| !tracked.source_path_hint.is_empty())
    );

    let v1 = source.version_metadata(&source_versions[0]);
    let v2 = source.version_metadata(&source_versions[1]);
    let v3 = source.version_metadata(&source_versions[2]);
    let v1_hashes = v1
        .files
        .iter()
        .map(|file| file.sha256.as_str())
        .collect::<std::collections::HashSet<_>>();
    let v3_hashes = v3
        .files
        .iter()
        .map(|file| file.sha256.as_str())
        .collect::<std::collections::HashSet<_>>();
    assert!(
        !v1_hashes.is_disjoint(&v3_hashes),
        "two versions should share a blob"
    );
    for file in &v2.files {
        assert!(
            !bundle
                .join("blobs")
                .join(&file.sha256[7..9])
                .join(&file.sha256[9..])
                .exists(),
            "retired-only payload {} must be omitted",
            file.sha256
        );
    }

    source.core.shutdown();
    let moved_source = source.repository.with_file_name("repository-moved-away");
    fs::rename(&source.repository, &moved_source).unwrap();
    assert!(!source.repository.exists());
    assert_eq!(
        TimelineCore::inspect_history_bundle(&bundle).unwrap(),
        summary
    );

    let destination = App::new();
    let destination_dirs = [tempdir().unwrap(), tempdir().unwrap()];
    for (index, dir) in destination_dirs.iter().enumerate() {
        fs::write(dir.path().join("keep.txt"), format!("unchanged-{index}")).unwrap();
    }
    let live_before = destination_dirs
        .iter()
        .map(|dir| snapshot_tree(dir.path()))
        .collect::<Vec<_>>();
    let paths_by_label = HashMap::from([
        ("Saves 1", destination_dirs[0].path()),
        ("Saves 2", destination_dirs[1].path()),
    ]);
    let mappings = summary
        .active_tracked_dirs
        .iter()
        .map(|tracked| {
            (
                tracked.uuid,
                paths_by_label[tracked.label.as_deref().unwrap()].to_path_buf(),
            )
        })
        .collect::<HashMap<_, _>>();

    let imported = destination
        .core
        .import_game_history(&bundle, &mappings)
        .unwrap();
    assert_eq!(imported.version_count, summary.version_count);
    assert_eq!(snapshot_tree(destination_dirs[0].path()), live_before[0]);
    assert_eq!(snapshot_tree(destination_dirs[1].path()), live_before[1]);
    let game = destination.core.get_games().unwrap().remove(0);
    assert!(game.active_slot.is_none());
    assert!(game.game.active_slot_id.is_none());
    assert_eq!(game.slots.len(), 2);
    let mut local_paths = game
        .tracked_dirs
        .iter()
        .map(|tracked| tracked.path.clone())
        .collect::<Vec<_>>();
    local_paths.sort();
    let mut expected_paths = destination_dirs
        .iter()
        .map(|dir| dir.path().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    expected_paths.sort();
    assert_eq!(local_paths, expected_paths);

    let game_dir = repository::game_dir(
        &destination.repository,
        game.game.folder_slug.as_deref().unwrap(),
    );
    let imported_tracks = fs::read_dir(repository::tracked_dirs_dir(&game_dir))
        .unwrap()
        .map(|entry| {
            repository::read_metadata_json::<TrackedDirMetadata>(&entry.unwrap().path()).unwrap()
        })
        .collect::<Vec<_>>();
    assert!(
        imported_tracks
            .iter()
            .all(|tracked| tracked.exclusions == vec!["ignored.tmp".to_string()])
    );
    assert!(imported_tracks.iter().all(|tracked| {
        tracked
            .label
            .as_deref()
            .is_some_and(|label| label.starts_with("Saves "))
    }));
    assert!(
        savegame_timeline_core::integrity::check_repository(&destination.repository, true)
            .issues
            .is_empty()
    );

    destination
        .core
        .import_game_history(&bundle, &HashMap::new())
        .unwrap();
    let slots = destination.games.list_slots(game.game.id).unwrap();
    let active_slot = slots.iter().find(|slot| !slot.versions.is_empty()).unwrap();
    let active_version = active_slot.versions[0].id;
    destination
        .versions
        .set_active_version(active_version)
        .unwrap();
    destination
        .games
        .set_active_slot(game.game.id, active_slot.slot.id)
        .unwrap();
    let before_reimport = destination.core.get_game(game.game.id).unwrap();
    let slot_id = before_reimport.active_slot.as_ref().unwrap().id;
    let version_id = before_reimport
        .active_slot
        .as_ref()
        .unwrap()
        .active_version_id;
    destination
        .core
        .import_game_history(&bundle, &HashMap::new())
        .unwrap();
    let after_reimport = destination.core.get_game(game.game.id).unwrap();
    assert_eq!(after_reimport.active_slot.as_ref().unwrap().id, slot_id);
    assert_eq!(
        after_reimport
            .active_slot
            .as_ref()
            .unwrap()
            .active_version_id,
        version_id
    );
    assert_eq!(
        savegame_timeline_core::integrity::check_repository(&destination.repository, true).issues,
        vec![]
    );
}

#[test]
fn rejects_corrupt_blobs_before_repository_or_database_changes() {
    let (source, _source_dirs, game_id, _) = complete_source();
    let bundle_parent = tempdir().unwrap();
    let bundle = bundle_parent.path().join("bundle");
    source.core.export_game_history(game_id, &bundle).unwrap();
    let summary = TimelineCore::inspect_history_bundle(&bundle).unwrap();
    let tracked_uuid = summary.active_tracked_dirs[0].uuid;
    let target = tempdir().unwrap();
    let mappings = HashMap::from([(tracked_uuid, target.path().to_path_buf())]);
    let destination = App::new();
    let initial_tree = snapshot_tree(&destination.repository);
    let blob_path = find_bundle_blob(&bundle);
    let good_bytes = fs::read(&blob_path).unwrap();

    fs::remove_file(&blob_path).unwrap();
    assert!(
        destination
            .core
            .import_game_history(&bundle, &mappings)
            .is_err()
    );
    assert!(destination.core.get_games().unwrap().is_empty());
    assert_eq!(snapshot_tree(&destination.repository), initial_tree);
    fs::write(&blob_path, &good_bytes).unwrap();

    fs::write(&blob_path, b"bad").unwrap();
    assert!(
        destination
            .core
            .import_game_history(&bundle, &mappings)
            .is_err()
    );
    assert!(destination.core.get_games().unwrap().is_empty());
    assert_eq!(snapshot_tree(&destination.repository), initial_tree);

    let mut wrong_hash = good_bytes.clone();
    wrong_hash[0] ^= 1;
    fs::write(&blob_path, wrong_hash).unwrap();
    assert!(
        destination
            .core
            .import_game_history(&bundle, &mappings)
            .is_err()
    );
    assert!(destination.core.get_games().unwrap().is_empty());
    assert_eq!(snapshot_tree(&destination.repository), initial_tree);
}

#[test]
fn failed_blob_copy_leaves_no_game_and_can_be_retried() {
    let (source, _source_dirs, game_id, versions) = complete_source();
    let bundle_parent = tempdir().unwrap();
    let bundle = bundle_parent.path().join("bundle");
    source.core.export_game_history(game_id, &bundle).unwrap();
    let summary = TimelineCore::inspect_history_bundle(&bundle).unwrap();
    let destination = App::new();
    let targets = [tempdir().unwrap(), tempdir().unwrap()];
    let mappings = summary
        .active_tracked_dirs
        .iter()
        .zip(&targets)
        .map(|(tracked, target)| (tracked.uuid, target.path().to_path_buf()))
        .collect::<HashMap<_, _>>();
    let version = source.version_metadata(&versions[0]);
    let first_file = &version.files[0];
    let destination_blob = savegame_timeline_core::content_store::blob_path(
        &destination.repository,
        &first_file.sha256,
    )
    .unwrap();
    fs::create_dir_all(destination_blob.parent().unwrap()).unwrap();
    fs::write(&destination_blob, vec![0; first_file.size as usize]).unwrap();
    assert!(
        destination
            .core
            .import_game_history(&bundle, &mappings)
            .is_err()
    );
    assert!(destination.core.get_games().unwrap().is_empty());
    assert_eq!(
        fs::read_dir(repository::games_dir(&destination.repository))
            .unwrap()
            .count(),
        0
    );

    fs::remove_file(destination_blob).unwrap();
    destination
        .core
        .import_game_history(&bundle, &mappings)
        .unwrap();
    assert_eq!(destination.core.get_games().unwrap().len(), 1);
    assert!(
        savegame_timeline_core::integrity::check_repository(&destination.repository, true)
            .issues
            .is_empty()
    );
}

#[test]
fn rejects_conflicting_local_version_identity_and_retirement_marker() {
    let (source, _source_dirs, game_id, _) = complete_source();
    let bundle_parent = tempdir().unwrap();
    let bundle = bundle_parent.path().join("bundle");
    source.core.export_game_history(game_id, &bundle).unwrap();
    let summary = TimelineCore::inspect_history_bundle(&bundle).unwrap();
    let targets = [tempdir().unwrap(), tempdir().unwrap()];
    let mappings = summary
        .active_tracked_dirs
        .iter()
        .zip(&targets)
        .map(|(tracked, target)| (tracked.uuid, target.path().to_path_buf()))
        .collect::<HashMap<_, _>>();
    let destination = App::new();
    destination
        .core
        .import_game_history(&bundle, &mappings)
        .unwrap();

    let bundle_game = fs::read_dir(bundle.join("games"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let bundle_slot = fs::read_dir(repository::slots_dir(&bundle_game))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let bundle_version = fs::read_dir(repository::versions_dir(&bundle_slot))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let local_game = destination
        .repository
        .join("games")
        .join(bundle_game.file_name().unwrap());
    let local_version = repository::version_dir(
        &repository::slot_dir(
            &local_game,
            bundle_slot.file_name().unwrap().to_str().unwrap(),
        ),
        bundle_version.file_name().unwrap().to_str().unwrap(),
    );
    let original: VersionMetadata = repository::read_metadata_json(&local_version).unwrap();
    let mut conflicting = original.clone();
    conflicting.reason = if conflicting.reason == VersionReason::Manual {
        VersionReason::Watcher
    } else {
        VersionReason::Manual
    };
    repository::write_metadata_json(&local_version, &conflicting).unwrap();

    let error = destination
        .core
        .import_game_history(&bundle, &HashMap::new())
        .unwrap_err()
        .to_string();
    assert!(error.contains("reused with different metadata"), "{error}");
    assert_eq!(destination.core.get_games().unwrap().len(), 1);
    let still_conflicting: VersionMetadata =
        repository::read_metadata_json(&local_version).unwrap();
    assert_eq!(still_conflicting, conflicting);

    repository::write_metadata_json(&local_version, &original).unwrap();

    let mut duplicate_metadata: repository::GameMetadata =
        repository::read_metadata_json(&local_game).unwrap();
    duplicate_metadata.folder_slug = "competing-slug".into();
    let duplicate_game = repository::games_dir(&destination.repository).join("competing-slug");
    repository::write_metadata_json(&duplicate_game, &duplicate_metadata).unwrap();
    let error = destination
        .core
        .import_game_history(&bundle, &HashMap::new())
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("already exists under folder slug"),
        "{error}"
    );
    fs::remove_dir_all(&duplicate_game).unwrap();

    let (marker_uuid, mut marker) =
        savegame_timeline_core::pruning::read_markers(&destination.repository)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
    marker.content_hash = "conflicting-marker".into();
    repository::write_json_file(
        &savegame_timeline_core::pruning::marker_path(&destination.repository, marker_uuid),
        &marker,
    )
    .unwrap();
    let error = destination
        .core
        .import_game_history(&bundle, &HashMap::new())
        .unwrap_err()
        .to_string();
    assert!(error.contains("Conflicting retirement marker"), "{error}");
    assert_eq!(destination.core.get_games().unwrap().len(), 1);
}

#[test]
fn rejects_unsafe_relative_paths_in_bundles() {
    let (source, _source_dirs, game_id, _) = complete_source();
    let bundle_parent = tempdir().unwrap();
    let bundle = bundle_parent.path().join("bundle");
    source.core.export_game_history(game_id, &bundle).unwrap();
    let bundle_game = fs::read_dir(bundle.join("games"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut version_with_files = None;
    for slot in fs::read_dir(repository::slots_dir(&bundle_game)).unwrap() {
        let slot = slot.unwrap().path();
        for version in fs::read_dir(repository::versions_dir(&slot)).unwrap() {
            let version = version.unwrap().path();
            let metadata: VersionMetadata = repository::read_metadata_json(&version).unwrap();
            if !metadata.files.is_empty() {
                version_with_files = Some((version, metadata));
                break;
            }
        }
        if version_with_files.is_some() {
            break;
        }
    }
    let (bundle_version, mut metadata) = version_with_files.unwrap();
    metadata.files[0].relative_path = "../outside-save.dat".into();
    repository::write_metadata_json(&bundle_version, &metadata).unwrap();

    let error = TimelineCore::inspect_history_bundle(&bundle)
        .unwrap_err()
        .to_string();
    assert!(error.contains("UnsafeRelativePath"), "{error}");
}

#[cfg(unix)]
#[test]
fn rejects_a_bundle_directory_symlink() {
    use std::os::unix::fs::symlink;

    let (source, _source_dirs, game_id, _) = complete_source();
    let bundle_parent = tempdir().unwrap();
    let bundle = bundle_parent.path().join("bundle");
    source.core.export_game_history(game_id, &bundle).unwrap();
    let link = bundle_parent.path().join("bundle-link");
    symlink(&bundle, &link).unwrap();

    assert!(TimelineCore::inspect_history_bundle(&link).is_err());
}

#[cfg(unix)]
#[test]
fn rejects_a_bundle_directory_symlink_alias() {
    use std::os::unix::fs::symlink;

    let (source, _source_dirs, game_id, _) = complete_source();
    let bundle_parent = tempdir().unwrap();
    let bundle = bundle_parent.path().join("bundle");
    source.core.export_game_history(game_id, &bundle).unwrap();
    let bundle_link = bundle_parent.path().join("bundle-link");
    symlink(&bundle, &bundle_link).unwrap();

    assert!(TimelineCore::inspect_history_bundle(&bundle_link).is_err());
}

#[test]
fn existing_game_import_can_retry_after_sqlite_failure() {
    let (source, source_dirs, game_id, _) = complete_source();
    let bundle_parent = tempdir().unwrap();
    let initial_bundle = bundle_parent.path().join("initial");
    source
        .core
        .export_game_history(game_id, &initial_bundle)
        .unwrap();
    let summary = TimelineCore::inspect_history_bundle(&initial_bundle).unwrap();
    let targets = [tempdir().unwrap(), tempdir().unwrap()];
    let mappings = summary
        .active_tracked_dirs
        .iter()
        .zip(&targets)
        .map(|(tracked, target)| (tracked.uuid, target.path().to_path_buf()))
        .collect::<HashMap<_, _>>();
    let destination = App::new();
    let destination_game_id = destination
        .core
        .import_game_history(&initial_bundle, &mappings)
        .unwrap();
    let _ = destination_game_id;

    fs::write(source_dirs[0].path().join("new-save.dat"), b"new history").unwrap();
    source.checkpoint(game_id, VersionReason::Manual);
    let updated_bundle = bundle_parent.path().join("updated");
    source
        .core
        .export_game_history(game_id, &updated_bundle)
        .unwrap();

    {
        let mut connection = destination.pool.get().unwrap();
        diesel::sql_query(
            "CREATE TRIGGER fail_history_insert BEFORE INSERT ON versions BEGIN SELECT RAISE(ABORT, 'injected failure'); END",
        )
        .execute(&mut connection)
        .unwrap();
    }
    assert!(
        destination
            .core
            .import_game_history(&updated_bundle, &HashMap::new())
            .is_err()
    );
    {
        let mut connection = destination.pool.get().unwrap();
        diesel::sql_query("DROP TRIGGER fail_history_insert")
            .execute(&mut connection)
            .unwrap();
    }

    destination
        .core
        .import_game_history(&updated_bundle, &HashMap::new())
        .unwrap();
    assert!(
        savegame_timeline_core::integrity::check_repository(&destination.repository, true)
            .issues
            .is_empty()
    );
}

#[test]
fn imports_multiple_retired_tracked_directories_idempotently() {
    let (source, _source_dirs, game_id, _) = complete_source();
    let bundle_parent = tempdir().unwrap();
    let bundle = bundle_parent.path().join("bundle");
    source.core.export_game_history(game_id, &bundle).unwrap();
    let game_dir = fs::read_dir(repository::games_dir(&bundle))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    for entry in fs::read_dir(repository::tracked_dirs_dir(&game_dir)).unwrap() {
        let dir = entry.unwrap().path();
        let mut tracked: TrackedDirMetadata = repository::read_metadata_json(&dir).unwrap();
        tracked.retired_at = Some(chrono::Utc::now());
        repository::write_metadata_json(&dir, &tracked).unwrap();
    }

    let destination = App::new();
    destination
        .core
        .import_game_history(&bundle, &HashMap::new())
        .unwrap();
    destination
        .core
        .import_game_history(&bundle, &HashMap::new())
        .unwrap();
    let mut connection = destination.pool.get().unwrap();
    use savegame_timeline_core::schema::tracked_dirs::dsl as tracked_dsl;
    let retired_paths = tracked_dsl::tracked_dirs
        .select(tracked_dsl::path)
        .filter(tracked_dsl::retired_at.is_not_null())
        .load::<String>(&mut connection)
        .unwrap();
    assert_eq!(retired_paths.len(), 2);
    assert_eq!(
        retired_paths
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        2
    );
}

#[test]
fn rejects_mapping_a_history_directory_used_by_another_game() {
    let (source, _source_dirs, game_id, _) = complete_source();
    let bundle_parent = tempdir().unwrap();
    let bundle = bundle_parent.path().join("bundle");
    source.core.export_game_history(game_id, &bundle).unwrap();
    let summary = TimelineCore::inspect_history_bundle(&bundle).unwrap();
    let shared = tempdir().unwrap();
    let separate = tempdir().unwrap();
    let destination = App::new();
    destination.add_game("Existing Game", &[&shared]);
    let mapping = HashMap::from([
        (summary.active_tracked_dirs[0].uuid, shared.path().join(".")),
        (
            summary.active_tracked_dirs[1].uuid,
            separate.path().to_path_buf(),
        ),
    ]);

    let before = snapshot_tree(&destination.repository);
    assert!(
        destination
            .core
            .import_game_history(&bundle, &mapping)
            .is_err()
    );
    assert_eq!(destination.core.get_games().unwrap().len(), 1);
    assert_eq!(snapshot_tree(&destination.repository), before);
}

#[test]
fn inspect_rejects_multi_node_parent_cycles() {
    let (source, _source_dirs, game_id, _) = complete_source();
    let bundle_parent = tempdir().unwrap();
    let bundle = bundle_parent.path().join("bundle");
    source.core.export_game_history(game_id, &bundle).unwrap();
    let game_dir = fs::read_dir(repository::games_dir(&bundle))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let slot_dir = fs::read_dir(repository::slots_dir(&game_dir))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|slot| {
            fs::read_dir(repository::versions_dir(slot))
                .unwrap()
                .count()
                >= 2
        })
        .unwrap();
    let mut versions = fs::read_dir(repository::versions_dir(&slot_dir))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    versions.sort();
    let mut first: VersionMetadata = repository::read_metadata_json(&versions[0]).unwrap();
    let mut second: VersionMetadata = repository::read_metadata_json(&versions[1]).unwrap();
    let first_uuid = first.uuid;
    let second_uuid = second.uuid;
    first.parent_version_uuid = Some(second_uuid);
    second.parent_version_uuid = Some(first_uuid);
    repository::write_metadata_json(&versions[0], &first).unwrap();
    repository::write_metadata_json(&versions[1], &second).unwrap();

    assert!(TimelineCore::inspect_history_bundle(&bundle).is_err());
}

#[test]
fn rejects_unsafe_mappings_and_refuses_existing_or_nested_export_destinations() {
    let (source, _source_dirs, game_id, _) = complete_source();
    let nested = source.repository.join("bundle-inside");
    assert!(source.core.export_game_history(game_id, &nested).is_err());
    assert!(!nested.exists());
    let existing = tempdir().unwrap();
    let sentinel = existing.path().join("keep.txt");
    fs::write(&sentinel, b"leave this directory alone").unwrap();
    assert!(
        source
            .core
            .export_game_history(game_id, existing.path())
            .is_err()
    );
    assert_eq!(fs::read(sentinel).unwrap(), b"leave this directory alone");
    assert!(
        source
            .core
            .export_game_history(game_id, source.repository.parent().unwrap())
            .is_err()
    );

    let bundle_parent = tempdir().unwrap();
    let bundle = bundle_parent.path().join("bundle");
    source.core.export_game_history(game_id, &bundle).unwrap();
    let summary = TimelineCore::inspect_history_bundle(&bundle).unwrap();
    let outside = tempdir().unwrap();
    let unsafe_path = outside.path().join("..").join("elsewhere");
    let mappings = HashMap::from([(summary.active_tracked_dirs[0].uuid, unsafe_path)]);
    let destination = App::new();
    let before = snapshot_tree(&destination.repository);
    assert!(
        destination
            .core
            .import_game_history(&bundle, &mappings)
            .is_err()
    );
    assert!(destination.core.get_games().unwrap().is_empty());
    assert_eq!(snapshot_tree(&destination.repository), before);

    let nested_bundle = destination.repository.join("nested-bundle");
    fs::rename(&bundle, &nested_bundle).unwrap();
    assert!(
        destination
            .core
            .import_game_history(&nested_bundle, &HashMap::new())
            .is_err()
    );
    assert!(destination.core.get_games().unwrap().is_empty());
}

#[test]
fn export_copy_failure_cleans_only_its_staging_directory_and_can_retry() {
    let (source, _source_dirs, game_id, versions) = complete_source();
    let version = source.version_metadata(&versions[0]);
    let blob = savegame_timeline_core::content_store::blob_path(
        &source.repository,
        &version.files[0].sha256,
    )
    .unwrap();
    let good = fs::read(&blob).unwrap();
    fs::write(&blob, b"corrupted").unwrap();
    let parent = tempdir().unwrap();
    let destination = parent.path().join("retry-bundle");
    assert!(
        source
            .core
            .export_game_history(game_id, &destination)
            .is_err()
    );
    assert!(!destination.exists());
    assert_eq!(fs::read_dir(parent.path()).unwrap().count(), 0);
    fs::write(&blob, &good).unwrap();
    source
        .core
        .export_game_history(game_id, &destination)
        .unwrap();
    assert!(TimelineCore::inspect_history_bundle(&destination).is_ok());
}

fn find_bundle_blob(bundle: &Path) -> PathBuf {
    fs::read_dir(bundle.join("blobs"))
        .unwrap()
        .flat_map(|shard| fs::read_dir(shard.unwrap().path()).unwrap())
        .next()
        .unwrap()
        .unwrap()
        .path()
}
