use crate::content_store::{blob_path, snapshot_content_hash, verify_blob};
use crate::repository::{
    self, GameMetadata, RepositoryMetadata, SlotMetadata, TrackedDirMetadata, VersionMetadata,
};
use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RepositoryIssueKind {
    UnsupportedFormat,
    TemporaryDirectory,
    MissingMetadata,
    InvalidMetadata,
    UnsafeRelativePath,
    MissingTrackedDirectory,
    ContentHashMismatch,
    MissingBlob,
    BlobSizeMismatch,
    BlobHashMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryIssue {
    pub kind: RepositoryIssueKind,
    pub path: PathBuf,
    pub detail: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepositoryIntegrityReport {
    pub issues: Vec<RepositoryIssue>,
}

pub fn check_repository(repository_root: &Path, deep: bool) -> RepositoryIntegrityReport {
    let mut checker = Checker {
        repository_root,
        deep,
        issues: Vec::new(),
        tracked_dirs: HashSet::new(),
    };
    checker.check();
    checker.issues.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.detail.cmp(&right.detail))
    });
    RepositoryIntegrityReport {
        issues: checker.issues,
    }
}

struct Checker<'a> {
    repository_root: &'a Path,
    deep: bool,
    issues: Vec<RepositoryIssue>,
    tracked_dirs: HashSet<(Uuid, Uuid)>,
}

impl Checker<'_> {
    fn check(&mut self) {
        match fs::symlink_metadata(self.repository_root) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => {
                self.issue(
                    RepositoryIssueKind::InvalidMetadata,
                    self.repository_root.to_path_buf(),
                    "repository root is not a directory",
                );
                return;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.issue(
                    RepositoryIssueKind::MissingMetadata,
                    repository::metadata_path(self.repository_root),
                    "repository root does not exist",
                );
                return;
            }
            Err(error) => {
                self.issue(
                    RepositoryIssueKind::InvalidMetadata,
                    self.repository_root.to_path_buf(),
                    format!("failed to inspect repository root: {error}"),
                );
                return;
            }
        }

        let root_metadata = self.load_metadata::<RepositoryMetadata>(self.repository_root);
        if let Some(metadata) = root_metadata
            && metadata.format_version != repository::REPOSITORY_FORMAT_VERSION
        {
            self.issue(
                RepositoryIssueKind::UnsupportedFormat,
                repository::metadata_path(self.repository_root),
                format!(
                    "repository format {} is unsupported; expected {}",
                    metadata.format_version,
                    repository::REPOSITORY_FORMAT_VERSION
                ),
            );
            return;
        }

        let games_dir = repository::games_dir(self.repository_root);
        if !self.is_directory_or_absent(&games_dir, "games directory") {
            return;
        }
        let Some(games) = self.read_child_directories(&games_dir) else {
            return;
        };
        for game_dir in games {
            self.check_game(&game_dir);
        }
    }

    fn check_game(&mut self, game_dir: &Path) {
        let Some(game) = self.load_metadata::<GameMetadata>(game_dir) else {
            return;
        };
        if game.entity != "game"
            || game.folder_slug != file_name(game_dir)
            || !valid_folder_slug(&game.folder_slug)
        {
            self.issue(
                RepositoryIssueKind::InvalidMetadata,
                repository::metadata_path(game_dir),
                "game metadata does not match its repository entry",
            );
            return;
        }

        let tracked_root = repository::tracked_dirs_dir(game_dir);
        if self.is_directory_or_absent(&tracked_root, "tracked-dirs directory")
            && let Some(tracked_dirs) = self.read_child_directories(&tracked_root)
        {
            for tracked_dir in tracked_dirs {
                self.check_tracked_dir(&tracked_dir, game.uuid);
            }
        }

        let slots_root = repository::slots_dir(game_dir);
        if !self.is_directory_or_absent(&slots_root, "slots directory") {
            return;
        }
        let Some(slots) = self.read_child_directories(&slots_root) else {
            return;
        };
        for slot_dir in slots {
            self.check_slot(&slot_dir, game.uuid);
        }
    }

    fn check_tracked_dir(&mut self, tracked_dir: &Path, game_uuid: Uuid) {
        let Some(metadata) = self.load_metadata::<TrackedDirMetadata>(tracked_dir) else {
            return;
        };
        if metadata.entity != "tracked_dir"
            || metadata.folder_slug != file_name(tracked_dir)
            || !valid_folder_slug(&metadata.folder_slug)
            || metadata.game_uuid != game_uuid
        {
            self.issue(
                RepositoryIssueKind::InvalidMetadata,
                repository::metadata_path(tracked_dir),
                "tracked directory metadata does not match its repository entry",
            );
            return;
        }
        self.tracked_dirs
            .insert((metadata.game_uuid, metadata.uuid));
    }

    fn check_slot(&mut self, slot_dir: &Path, game_uuid: Uuid) {
        let Some(slot) = self.load_metadata::<SlotMetadata>(slot_dir) else {
            return;
        };
        if slot.entity != "slot"
            || slot.folder_slug != file_name(slot_dir)
            || !valid_folder_slug(&slot.folder_slug)
            || slot.game_uuid != game_uuid
        {
            self.issue(
                RepositoryIssueKind::InvalidMetadata,
                repository::metadata_path(slot_dir),
                "slot metadata does not match its repository entry",
            );
            return;
        }

        let versions_root = repository::versions_dir(slot_dir);
        if !self.is_directory_or_absent(&versions_root, "versions directory") {
            return;
        }
        let Some(entries) = self.read_child_entries(&versions_root) else {
            return;
        };
        for entry in entries {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                self.issue(
                    RepositoryIssueKind::InvalidMetadata,
                    path,
                    "failed to inspect version entry",
                );
                continue;
            };
            if file_type.is_symlink() {
                self.issue(
                    RepositoryIssueKind::InvalidMetadata,
                    path,
                    "version entry is a symlink",
                );
                continue;
            }
            if !file_type.is_dir() {
                continue;
            }
            if is_temporary_version_dir(&path) {
                self.issue(
                    RepositoryIssueKind::TemporaryDirectory,
                    path,
                    "temporary version directory was left behind",
                );
                continue;
            }
            self.check_version(&path, game_uuid, slot.uuid);
        }
    }

    fn check_version(&mut self, version_dir: &Path, game_uuid: Uuid, slot_uuid: Uuid) {
        let Some(metadata) = self.load_metadata::<VersionMetadata>(version_dir) else {
            return;
        };
        let metadata_path = repository::metadata_path(version_dir);
        if metadata.entity != "version"
            || metadata.folder_slug != file_name(version_dir)
            || !valid_folder_slug(&metadata.folder_slug)
            || metadata.game_uuid != game_uuid
            || metadata.slot_uuid != Some(slot_uuid)
        {
            self.issue(
                RepositoryIssueKind::InvalidMetadata,
                metadata_path.clone(),
                "version metadata does not match its repository entry",
            );
            return;
        }

        let mut canonical_files = metadata.files.clone();
        sort_manifest_files(&mut canonical_files);
        if canonical_files != metadata.files {
            self.issue(
                RepositoryIssueKind::InvalidMetadata,
                metadata_path.clone(),
                "version manifest is not in canonical order",
            );
        }
        match snapshot_content_hash(&metadata.files) {
            Ok(actual) if actual != metadata.content_hash => self.issue(
                RepositoryIssueKind::ContentHashMismatch,
                metadata_path.clone(),
                format!(
                    "manifest content hash is {}, expected {}",
                    metadata.content_hash, actual
                ),
            ),
            Err(error) => self.issue(
                RepositoryIssueKind::ContentHashMismatch,
                metadata_path.clone(),
                format!("failed to calculate manifest content hash: {error}"),
            ),
            Ok(_) => {}
        }

        for file in &metadata.files {
            if !valid_relative_path(&file.relative_path) {
                self.issue(
                    RepositoryIssueKind::UnsafeRelativePath,
                    metadata_path.clone(),
                    format!("unsafe manifest relative path {}", file.relative_path),
                );
            }
            if !self
                .tracked_dirs
                .contains(&(metadata.game_uuid, file.tracked_dir_uuid))
            {
                self.issue(
                    RepositoryIssueKind::MissingTrackedDirectory,
                    metadata_path.clone(),
                    format!(
                        "manifest references missing tracked directory {}",
                        file.tracked_dir_uuid
                    ),
                );
            }
            let blob = match blob_path(self.repository_root, &file.sha256) {
                Ok(path) => path,
                Err(error) => {
                    self.issue(
                        RepositoryIssueKind::InvalidMetadata,
                        metadata_path.clone(),
                        format!("invalid blob hash {}: {error}", file.sha256),
                    );
                    continue;
                }
            };
            self.check_blob(&blob, file, &metadata_path);
        }
    }

    fn check_blob(
        &mut self,
        blob: &Path,
        file: &crate::repository::VersionFileMetadata,
        metadata_path: &Path,
    ) {
        let metadata = match fs::symlink_metadata(blob) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.issue(
                    RepositoryIssueKind::MissingBlob,
                    blob.to_path_buf(),
                    format!(
                        "referenced blob is missing (from {})",
                        metadata_path.display()
                    ),
                );
                return;
            }
            Err(error) => {
                self.issue(
                    RepositoryIssueKind::MissingBlob,
                    blob.to_path_buf(),
                    format!("failed to inspect referenced blob: {error}"),
                );
                return;
            }
        };
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            self.issue(
                RepositoryIssueKind::MissingBlob,
                blob.to_path_buf(),
                "referenced blob is not a regular file",
            );
            return;
        }
        if !self.deep {
            return;
        }
        if metadata.len() != file.size {
            self.issue(
                RepositoryIssueKind::BlobSizeMismatch,
                blob.to_path_buf(),
                format!("blob has size {}, expected {}", metadata.len(), file.size),
            );
            return;
        }
        if let Err(error) = verify_blob(self.repository_root, &file.sha256, file.size) {
            self.issue(
                RepositoryIssueKind::BlobHashMismatch,
                blob.to_path_buf(),
                format!("blob hash verification failed: {error}"),
            );
        }
    }

    fn load_metadata<T>(&mut self, entity_dir: &Path) -> Option<T>
    where
        T: serde::de::DeserializeOwned,
    {
        let path = repository::metadata_path(entity_dir);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
            Ok(_) => {
                self.issue(
                    RepositoryIssueKind::InvalidMetadata,
                    path,
                    "metadata path is not a regular file",
                );
                return None;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.issue(
                    RepositoryIssueKind::MissingMetadata,
                    path,
                    "metadata.json is missing",
                );
                return None;
            }
            Err(error) => {
                self.issue(
                    RepositoryIssueKind::InvalidMetadata,
                    path,
                    format!("failed to inspect metadata: {error}"),
                );
                return None;
            }
        }
        match repository::read_metadata_json(entity_dir) {
            Ok(metadata) => Some(metadata),
            Err(error) => {
                self.issue(
                    RepositoryIssueKind::InvalidMetadata,
                    path,
                    format!("failed to parse metadata: {error}"),
                );
                None
            }
        }
    }

    fn is_directory_or_absent(&mut self, path: &Path, label: &str) -> bool {
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => true,
            Ok(_) => {
                self.issue(
                    RepositoryIssueKind::InvalidMetadata,
                    path.to_path_buf(),
                    format!("{label} is not a directory"),
                );
                false
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
            Err(error) => {
                self.issue(
                    RepositoryIssueKind::InvalidMetadata,
                    path.to_path_buf(),
                    format!("failed to inspect {label}: {error}"),
                );
                false
            }
        }
    }

    fn read_child_directories(&mut self, parent: &Path) -> Option<Vec<PathBuf>> {
        let entries = self.read_child_entries(parent)?;
        let mut directories = Vec::new();
        for entry in entries {
            let path = entry.path();
            match entry.file_type() {
                Ok(file_type) if file_type.is_dir() && !file_type.is_symlink() => {
                    directories.push(path)
                }
                Ok(file_type) if file_type.is_symlink() => self.issue(
                    RepositoryIssueKind::InvalidMetadata,
                    path,
                    "repository entry is a symlink",
                ),
                Ok(_) => {}
                Err(error) => self.issue(
                    RepositoryIssueKind::InvalidMetadata,
                    path,
                    format!("failed to inspect repository entry: {error}"),
                ),
            }
        }
        Some(directories)
    }

    fn read_child_entries(&mut self, parent: &Path) -> Option<Vec<fs::DirEntry>> {
        let entries = match fs::read_dir(parent) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Some(Vec::new());
            }
            Err(error) => {
                self.issue(
                    RepositoryIssueKind::InvalidMetadata,
                    parent.to_path_buf(),
                    format!("failed to read repository directory: {error}"),
                );
                return None;
            }
        };
        let mut entries = entries.flatten().collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.path());
        Some(entries)
    }

    fn issue(&mut self, kind: RepositoryIssueKind, path: PathBuf, detail: impl Into<String>) {
        self.issues.push(RepositoryIssue {
            kind,
            path,
            detail: detail.into(),
        });
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string()
}

fn valid_folder_slug(value: &str) -> bool {
    let path = Path::new(value);
    let mut components = path.components();
    matches!(components.next(), Some(Component::Normal(_)))
        && components.next().is_none()
        && !value.is_empty()
        && value != "."
        && value != ".."
        && !value.contains('/')
        && !value.contains('\\')
}

fn is_temporary_version_dir(path: &Path) -> bool {
    let name = file_name(path);
    name.contains(".tmp-")
}

fn valid_relative_path(value: &str) -> bool {
    if value.contains('\\') || value.contains('\0') {
        return false;
    }
    let path = Path::new(value);
    let mut has_normal_component = false;
    for component in path.components() {
        match component {
            Component::Normal(_) => has_normal_component = true,
            Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir | Component::ParentDir => return false,
        }
    }
    has_normal_component && !path.is_absolute() && !path.has_root()
}

fn sort_manifest_files(files: &mut [crate::repository::VersionFileMetadata]) {
    files.sort_by(|left, right| {
        left.tracked_dir_uuid
            .cmp(&right.tracked_dir_uuid)
            .then_with(|| left.relative_path.cmp(&right.relative_path))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content_store::{blob_path, snapshot_content_hash};
    use crate::repository::{
        self, GameMetadata, RepositoryMetadata, SlotMetadata, TrackedDirMetadata,
        VersionFileMetadata, VersionMetadata, VersionReason,
    };
    use chrono::{DateTime, Utc};
    use sha2::Digest;
    use std::fs;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;
    use uuid::Uuid;

    #[test]
    fn reports_tmp_missing_metadata_bad_manifest_and_corrupt_blob_without_deleting_anything() {
        let fixture = IntegrityFixture::new();
        fixture.add_tmp_version_dir();
        fixture.add_version_without_metadata();
        fixture.add_version_with_wrong_content_hash();
        fixture.corrupt_referenced_blob();

        let report = check_repository(fixture.root(), true);
        assert!(report.has_kind(RepositoryIssueKind::TemporaryDirectory));
        assert!(report.has_kind(RepositoryIssueKind::MissingMetadata));
        assert!(report.has_kind(RepositoryIssueKind::ContentHashMismatch));
        assert!(report.has_kind(RepositoryIssueKind::BlobHashMismatch));
        assert!(fixture.tmp_version_dir().exists());
        assert!(fixture.corrupt_blob().exists());
    }

    #[test]
    fn shallow_check_does_not_rehash_blob_bytes_and_unknown_files_are_ignored() {
        let fixture = IntegrityFixture::valid();
        fs::write(fixture.root().join("state (1).json"), b"cloud conflict").unwrap();
        fixture.corrupt_referenced_blob();
        let report = check_repository(fixture.root(), false);
        assert!(!report.has_kind(RepositoryIssueKind::BlobHashMismatch));
        assert!(report.issues.is_empty());
    }

    #[test]
    fn unsupported_format_stops_descendant_interpretation() {
        let fixture = IntegrityFixture::valid();
        fixture.set_format_version(1);
        fixture.add_version_without_metadata();

        let report = check_repository(fixture.root(), true);

        assert_eq!(report.issues.len(), 1);
        assert_eq!(
            report.issues[0].kind,
            RepositoryIssueKind::UnsupportedFormat
        );
    }

    #[test]
    fn reports_remaining_issue_kinds() {
        type IssueCase = (&'static str, fn(&IntegrityFixture), RepositoryIssueKind);
        let cases: [IssueCase; 5] = [
            (
                "invalid metadata",
                IntegrityFixture::add_malformed_version_metadata,
                RepositoryIssueKind::InvalidMetadata,
            ),
            (
                "unsafe relative path",
                IntegrityFixture::add_unsafe_relative_path,
                RepositoryIssueKind::UnsafeRelativePath,
            ),
            (
                "missing tracked directory",
                IntegrityFixture::remove_tracked_directory_reference,
                RepositoryIssueKind::MissingTrackedDirectory,
            ),
            (
                "missing blob",
                IntegrityFixture::remove_referenced_blob,
                RepositoryIssueKind::MissingBlob,
            ),
            (
                "blob size mismatch",
                IntegrityFixture::write_wrong_size_blob,
                RepositoryIssueKind::BlobSizeMismatch,
            ),
        ];

        for (name, mutate, kind) in cases {
            let fixture = IntegrityFixture::valid();
            mutate(&fixture);
            let report = check_repository(fixture.root(), true);
            assert!(report.has_kind(kind), "{name}: {report:?}");
        }
    }

    #[test]
    fn malformed_sibling_does_not_hide_valid_sibling_findings() {
        let fixture = IntegrityFixture::valid();
        fixture.add_malformed_version_metadata();
        fixture.corrupt_referenced_blob();

        let report = check_repository(fixture.root(), true);

        assert!(report.has_kind(RepositoryIssueKind::InvalidMetadata));
        assert!(report.has_kind(RepositoryIssueKind::BlobHashMismatch));
    }

    #[test]
    fn rejects_noncanonical_manifest_order() {
        let fixture = IntegrityFixture::valid();
        fixture.add_noncanonical_manifest_order();

        let report = check_repository(fixture.root(), true);

        assert!(report.has_kind(RepositoryIssueKind::InvalidMetadata));
        assert!(!report.has_kind(RepositoryIssueKind::ContentHashMismatch));
    }

    #[test]
    fn issues_are_stable_and_sorted_by_path_then_kind() {
        let fixture = IntegrityFixture::valid();
        fixture.add_tmp_version_dir();
        fixture.add_version_without_metadata();
        fixture.add_malformed_version_metadata();
        fixture.corrupt_referenced_blob();

        let first = check_repository(fixture.root(), true);
        let second = check_repository(fixture.root(), true);

        assert_eq!(first, second);
        assert!(first.issues.windows(2).all(|pair| {
            pair[0]
                .path
                .cmp(&pair[1].path)
                .then_with(|| pair[0].kind.cmp(&pair[1].kind))
                .is_le()
        }));
    }

    #[test]
    fn checking_does_not_change_any_repository_bytes() {
        let fixture = IntegrityFixture::new();
        fixture.add_tmp_version_dir();
        fixture.add_version_without_metadata();
        fixture.add_malformed_version_metadata();
        fixture.add_noncanonical_manifest_order();
        fixture.corrupt_referenced_blob();
        let before = snapshot_tree(fixture.root());

        let _ = check_repository(fixture.root(), true);

        assert_eq!(before, snapshot_tree(fixture.root()));
    }

    fn snapshot_tree(root: &Path) -> Vec<(String, Vec<u8>)> {
        fn visit(root: &Path, current: &Path, files: &mut Vec<(String, Vec<u8>)>) {
            let mut entries = fs::read_dir(current)
                .unwrap()
                .map(|entry| entry.unwrap())
                .collect::<Vec<_>>();
            entries.sort_by_key(|entry| entry.path());
            for entry in entries {
                let path = entry.path();
                if entry.file_type().unwrap().is_dir() {
                    visit(root, &path, files);
                } else if entry.file_type().unwrap().is_file() {
                    files.push((
                        path.strip_prefix(root)
                            .unwrap()
                            .to_string_lossy()
                            .replace('\\', "/"),
                        fs::read(path).unwrap(),
                    ));
                }
            }
        }

        let mut files = Vec::new();
        visit(root, root, &mut files);
        files.sort_by(|left, right| left.0.cmp(&right.0));
        files
    }

    struct IntegrityFixture {
        root: TempDir,
        version_dir: PathBuf,
        blob: PathBuf,
    }

    impl IntegrityFixture {
        fn new() -> Self {
            Self::valid()
        }

        fn valid() -> Self {
            let root = tempfile::tempdir().unwrap();
            let game_uuid = Uuid::from_u128(1);
            let tracked_uuid = Uuid::from_u128(2);
            let slot_uuid = Uuid::from_u128(3);
            let version_uuid = Uuid::from_u128(4);
            let game_slug = "fixture__g_00000000";
            let tracked_slug = "saves__d_00000000";
            let slot_slug = "default__s_00000000";
            let version_slug = "v001__v_00000000";
            let now = DateTime::<Utc>::from_timestamp(0, 0).unwrap();
            let bytes = b"abcdefgh";
            let hash = format!("sha256:{:x}", sha2::Sha256::digest(bytes));
            let file = VersionFileMetadata {
                tracked_dir_uuid: tracked_uuid,
                relative_path: "save.dat".to_string(),
                size: bytes.len() as u64,
                sha256: hash.clone(),
            };
            let content_hash = snapshot_content_hash(std::slice::from_ref(&file)).unwrap();

            repository::write_metadata_json(
                root.path(),
                &RepositoryMetadata {
                    format_version: repository::REPOSITORY_FORMAT_VERSION,
                    created_at: now,
                    app: "savegame-timeline".to_string(),
                },
            )
            .unwrap();
            fs::create_dir_all(repository::blobs_dir(root.path())).unwrap();
            let game_dir = repository::game_dir(root.path(), game_slug);
            repository::write_metadata_json(
                &game_dir,
                &GameMetadata {
                    entity: "game".to_string(),
                    uuid: game_uuid,
                    title: "Fixture".to_string(),
                    folder_slug: game_slug.to_string(),
                    active_slot_uuid: Some(slot_uuid),
                    icon: None,
                    created_at: now,
                    updated_at: now,
                },
            )
            .unwrap();
            let tracked_dir = repository::tracked_dir_dir(&game_dir, tracked_slug);
            repository::write_metadata_json(
                &tracked_dir,
                &TrackedDirMetadata {
                    entity: "tracked_dir".to_string(),
                    uuid: tracked_uuid,
                    game_uuid,
                    path: "/fixture/saves".to_string(),
                    label: None,
                    exclusions: Vec::new(),
                    folder_slug: tracked_slug.to_string(),
                    retired_at: None,
                    created_at: now,
                    updated_at: now,
                },
            )
            .unwrap();
            let slot_dir = repository::slot_dir(&game_dir, slot_slug);
            repository::write_metadata_json(
                &slot_dir,
                &SlotMetadata {
                    entity: "slot".to_string(),
                    uuid: slot_uuid,
                    game_uuid,
                    name: "Default".to_string(),
                    folder_slug: slot_slug.to_string(),
                    active_version_uuid: Some(version_uuid),
                    created_at: now,
                    updated_at: now,
                },
            )
            .unwrap();
            let version_dir = repository::version_dir(&slot_dir, version_slug);
            repository::write_metadata_json(
                &version_dir,
                &VersionMetadata {
                    entity: "version".to_string(),
                    uuid: version_uuid,
                    game_uuid,
                    slot_uuid: Some(slot_uuid),
                    version_number: 1,
                    folder_slug: version_slug.to_string(),
                    parent_version_uuid: None,
                    tag: None,
                    reason: VersionReason::Initial,
                    created_at: now,
                    content_hash,
                    files: vec![file],
                },
            )
            .unwrap();
            let blob = blob_path(root.path(), &hash).unwrap();
            fs::create_dir_all(blob.parent().unwrap()).unwrap();
            fs::write(&blob, bytes).unwrap();

            Self {
                root,
                version_dir,
                blob,
            }
        }

        fn root(&self) -> &Path {
            self.root.path()
        }

        fn set_format_version(&self, format_version: u32) {
            let mut metadata: RepositoryMetadata =
                repository::read_metadata_json(self.root()).unwrap();
            metadata.format_version = format_version;
            repository::write_metadata_json(self.root(), &metadata).unwrap();
        }

        fn add_malformed_version_metadata(&self) {
            let dir = self.version_dir.parent().unwrap().join("v005__v_00000000");
            fs::create_dir_all(&dir).unwrap();
            fs::write(repository::metadata_path(&dir), b"not json").unwrap();
        }

        fn rewrite_version_metadata(&self, mutate: impl FnOnce(&mut VersionMetadata)) {
            let mut metadata: VersionMetadata =
                repository::read_metadata_json(&self.version_dir).unwrap();
            mutate(&mut metadata);
            repository::write_metadata_json(&self.version_dir, &metadata).unwrap();
        }

        fn add_unsafe_relative_path(&self) {
            self.rewrite_version_metadata(|metadata| {
                metadata.files[0].relative_path = "../outside.dat".to_string();
            });
        }

        fn remove_tracked_directory_reference(&self) {
            self.rewrite_version_metadata(|metadata| {
                metadata.files[0].tracked_dir_uuid = Uuid::from_u128(99);
            });
        }

        fn remove_referenced_blob(&self) {
            fs::remove_file(&self.blob).unwrap();
        }

        fn write_wrong_size_blob(&self) {
            fs::write(&self.blob, b"short").unwrap();
        }

        fn add_noncanonical_manifest_order(&self) {
            self.rewrite_version_metadata(|metadata| {
                let mut second = metadata.files[0].clone();
                second.relative_path = "a.dat".to_string();
                let first = metadata.files[0].clone();
                metadata.files = vec![first, second];
                metadata.content_hash = snapshot_content_hash(&metadata.files).unwrap();
            });
        }

        fn add_tmp_version_dir(&self) {
            let temp_dir = self
                .version_dir
                .parent()
                .unwrap()
                .join("v002__v_00000000.tmp-deadbeef");
            fs::create_dir_all(&temp_dir).unwrap();
            fs::write(temp_dir.join("partial"), b"partial").unwrap();
        }

        fn add_version_without_metadata(&self) {
            let dir = self.version_dir.parent().unwrap().join("v003__v_00000000");
            fs::create_dir_all(dir).unwrap();
        }

        fn add_version_with_wrong_content_hash(&self) {
            let mut metadata: VersionMetadata =
                repository::read_metadata_json(&self.version_dir).unwrap();
            metadata.uuid = Uuid::from_u128(5);
            metadata.folder_slug = "v004__v_00000000".to_string();
            metadata.content_hash = format!("sha256:{}", "0".repeat(64));
            let dir = self
                .version_dir
                .parent()
                .unwrap()
                .join(&metadata.folder_slug);
            repository::write_metadata_json(&dir, &metadata).unwrap();
        }

        fn corrupt_referenced_blob(&self) {
            fs::write(&self.blob, b"12345678").unwrap();
        }

        fn tmp_version_dir(&self) -> PathBuf {
            self.version_dir
                .parent()
                .unwrap()
                .join("v002__v_00000000.tmp-deadbeef")
        }

        fn corrupt_blob(&self) -> PathBuf {
            self.blob.clone()
        }
    }

    impl RepositoryIntegrityReport {
        fn has_kind(&self, kind: RepositoryIssueKind) -> bool {
            self.issues.iter().any(|issue| issue.kind == kind)
        }
    }
}
