use anyhow::{Context, Result, anyhow};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::models::TrackedDir;
use crate::repository::VersionFileMetadata;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct CapturedSnapshot {
    pub content_hash: String,
    pub files: Vec<VersionFileMetadata>,
}

pub fn capture_snapshot(
    repository_root: &Path,
    tracked_dirs: &[TrackedDir],
) -> Result<CapturedSnapshot> {
    let blobs_root = repository_root.join("blobs");
    fs::create_dir_all(&blobs_root).with_context(|| {
        format!(
            "failed to create blob directory at {}",
            blobs_root.display()
        )
    })?;

    let mut files = Vec::new();
    for tracked_dir in tracked_dirs {
        let tracked_dir_uuid = tracked_dir
            .uuid
            .as_deref()
            .ok_or_else(|| anyhow!("tracked directory is missing a UUID"))
            .and_then(|value| {
                Uuid::parse_str(value)
                    .with_context(|| format!("invalid tracked directory UUID {value}"))
            })?;
        let root = Path::new(&tracked_dir.path);
        let exclusions = crate::exclusions::parse(&tracked_dir.exclusions);
        let mut paths = Vec::new();
        collect_regular_files(root, &exclusions, Path::new(""), true, &mut paths)?;
        paths.sort();

        for path in paths {
            let relative_path = path
                .strip_prefix(root)
                .with_context(|| {
                    format!(
                        "failed to make {} relative to {}",
                        path.display(),
                        root.display()
                    )
                })?
                .to_string_lossy()
                .replace('\\', "/");
            let relative_path = Path::new(&relative_path);
            if crate::exclusions::is_excluded(&exclusions, relative_path) {
                continue;
            }

            let (digest, size) = hash_file(&path)?;
            let hash = format_hash(&digest);
            let blob = blob_path(repository_root, &hash)?;
            write_blob(&path, &blob, &hash, size)?;
            files.push(VersionFileMetadata {
                tracked_dir_uuid,
                relative_path: relative_path.to_string_lossy().into_owned(),
                size,
                sha256: hash,
            });
        }
    }

    files.sort_by(|a, b| {
        a.tracked_dir_uuid
            .cmp(&b.tracked_dir_uuid)
            .then_with(|| a.relative_path.cmp(&b.relative_path))
    });
    let manifest = serde_json::to_vec(&files).context("failed to serialize snapshot manifest")?;
    let content_hash = format_hash(&Sha256::digest(manifest));
    Ok(CapturedSnapshot {
        content_hash,
        files,
    })
}

pub fn blob_path(repository_root: &Path, hash: &str) -> Result<PathBuf> {
    let digest = parse_hash(hash)?;
    Ok(repository_root
        .join("blobs")
        .join(&digest[..2])
        .join(&digest[2..]))
}

pub fn verify_blob(repository_root: &Path, hash: &str, expected_size: u64) -> Result<PathBuf> {
    let path = blob_path(repository_root, hash)?;
    let metadata = fs::metadata(&path)
        .with_context(|| format!("failed to stat blob at {}", path.display()))?;
    if !metadata.is_file() {
        return Err(anyhow!(
            "blob path is not a regular file: {}",
            path.display()
        ));
    }
    if metadata.len() != expected_size {
        return Err(anyhow!(
            "blob {} has size {}, expected {}",
            hash,
            metadata.len(),
            expected_size
        ));
    }
    let (digest, _) = hash_file(&path)?;
    if format_hash(&digest) != hash {
        return Err(anyhow!("blob {} failed SHA-256 verification", hash));
    }
    Ok(path)
}

fn collect_regular_files(
    current: &Path,
    exclusions: &[glob::Pattern],
    relative: &Path,
    is_root: bool,
    files: &mut Vec<PathBuf>,
) -> Result<()> {
    let entries = match fs::read_dir(current) {
        Ok(entries) => entries,
        Err(error) if is_root && error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error).with_context(|| {
                format!("failed to read tracked directory {}", current.display())
            });
        }
    };

    for entry in entries {
        let entry = entry?;
        let file_name = entry.file_name();
        if crate::fs_utils::is_excluded_file(&file_name.to_string_lossy()) {
            continue;
        }
        let entry_relative = relative.join(&file_name);
        if crate::exclusions::is_excluded(exclusions, &entry_relative) {
            continue;
        }
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .with_context(|| format!("failed to stat tracked path {}", path.display()))?;
        if metadata.is_dir() {
            collect_regular_files(&path, exclusions, &entry_relative, false, files)?;
        } else if metadata.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<([u8; 32], u64)> {
    let mut file =
        File::open(path).with_context(|| format!("failed to open file {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut size = 0;
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("failed to read file {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        size += read as u64;
    }
    Ok((hasher.finalize().into(), size))
}

fn write_blob(source: &Path, destination: &Path, hash: &str, expected_size: u64) -> Result<()> {
    if destination.exists() {
        verify_existing_blob(destination, hash, expected_size)?;
        return Ok(());
    }

    let parent = destination
        .parent()
        .ok_or_else(|| anyhow!("blob path has no parent: {}", destination.display()))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create blob shard {}", parent.display()))?;
    let temp = parent.join(format!(
        ".{}.tmp-{}",
        destination.file_name().unwrap().to_string_lossy(),
        Uuid::new_v4()
    ));
    let result = (|| -> Result<()> {
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .with_context(|| format!("failed to create blob temp file {}", temp.display()))?;
        let mut input = File::open(source)
            .with_context(|| format!("failed to reopen source file {}", source.display()))?;
        let mut buffer = [0_u8; 64 * 1024];
        let mut hasher = Sha256::new();
        let mut size = 0;
        loop {
            let read = input
                .read(&mut buffer)
                .with_context(|| format!("failed to read source file {}", source.display()))?;
            if read == 0 {
                break;
            }
            output.write_all(&buffer[..read])?;
            hasher.update(&buffer[..read]);
            size += read as u64;
        }
        if size != expected_size || format_hash(&hasher.finalize()) != hash {
            return Err(anyhow!(
                "source file changed during blob capture: {}",
                source.display()
            ));
        }
        output.sync_all()?;
        drop(output);
        publish_blob_no_replace(&temp, destination, hash, expected_size)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn publish_blob_no_replace(
    temp: &Path,
    destination: &Path,
    hash: &str,
    expected_size: u64,
) -> Result<()> {
    match atomic_rename_no_replace(temp, destination) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let verification = verify_existing_blob(destination, hash, expected_size);
            let _ = fs::remove_file(temp);
            verification
        }
        Err(error) => Err(error.into()),
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn atomic_rename_no_replace(source: &Path, destination: &Path) -> io::Result<()> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};

    renameat_with(CWD, source, CWD, destination, RenameFlags::NOREPLACE).map_err(Into::into)
}

#[cfg(windows)]
pub(crate) fn atomic_rename_no_replace(source: &Path, destination: &Path) -> io::Result<()> {
    use std::iter;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
pub(crate) fn atomic_rename_no_replace(_: &Path, _: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "atomic no-replace rename is only implemented for Linux and Windows",
    ))
}

fn verify_existing_blob(path: &Path, hash: &str, expected_size: u64) -> Result<()> {
    let metadata = fs::metadata(path)?;
    if metadata.len() != expected_size {
        return Err(anyhow!(
            "existing blob has unexpected size: {}",
            path.display()
        ));
    }
    let (digest, _) = hash_file(path)?;
    if format_hash(&digest) != hash {
        return Err(anyhow!(
            "existing blob failed SHA-256 verification: {}",
            path.display()
        ));
    }
    Ok(())
}

fn parse_hash(hash: &str) -> Result<&str> {
    let digest = hash
        .strip_prefix("sha256:")
        .ok_or_else(|| anyhow!("unsupported blob hash algorithm"))?;
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(anyhow!("invalid SHA-256 blob hash"));
    }
    Ok(digest)
}

fn format_hash(digest: &[u8]) -> String {
    let mut hash = String::from("sha256:");
    for byte in digest {
        hash.push_str(&format!("{byte:02x}"));
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::TrackedDir;
    use std::fs;
    use std::path::Path;
    use tempfile::{TempDir, tempdir};
    use uuid::Uuid;

    fn tracked_dir(path: &Path, uuid: Uuid, exclusions: &str) -> TrackedDir {
        TrackedDir {
            id: 1,
            uuid: Some(uuid.to_string()),
            folder_slug: Some("fixture__d_00000000".to_string()),
            game_id: 1,
            path: path.to_string_lossy().into_owned(),
            exclusions: exclusions.to_string(),
        }
    }

    fn count_regular_files(path: &Path) -> usize {
        let mut count = 0;
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    count += count_regular_files(&path);
                } else if path.is_file() {
                    count += 1;
                }
            }
        }
        count
    }

    fn fixture_tracked_dir(name: &str, bytes: &[u8], uuid: Uuid) -> (TempDir, TrackedDir) {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(name), bytes).unwrap();
        let tracked = tracked_dir(dir.path(), uuid, "[]");
        (dir, tracked)
    }

    #[test]
    fn captures_files_into_sharded_blobs_once_and_builds_a_stable_manifest() {
        let repo = tempdir().unwrap();
        let live = tempdir().unwrap();
        fs::write(live.path().join("save.dat"), b"same bytes").unwrap();
        fs::write(live.path().join("ignored.tmp"), b"ignored").unwrap();
        fs::write(live.path().join("ignored.log"), b"globally ignored").unwrap();
        let ignored_dir = live.path().join("ignored.cache");
        fs::create_dir_all(&ignored_dir).unwrap();
        fs::write(ignored_dir.join("nested.dat"), b"globally ignored subtree").unwrap();
        let dir = tracked_dir(live.path(), Uuid::from_u128(1), "[\"*.tmp\"]");

        let first = capture_snapshot(repo.path(), std::slice::from_ref(&dir)).unwrap();
        let second = capture_snapshot(repo.path(), &[dir]).unwrap();

        assert_eq!(first, second);
        assert_eq!(first.files.len(), 1);
        let hash = &first.files[0].sha256;
        let digest = hash.strip_prefix("sha256:").unwrap();
        assert_eq!(digest.len(), 64);
        assert!(
            digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        );
        assert_eq!(count_regular_files(&repo.path().join("blobs")), 1);
        assert_eq!(
            blob_path(repo.path(), hash).unwrap(),
            repo.path()
                .join("blobs")
                .join(&digest[..2])
                .join(&digest[2..])
        );
        assert!(
            blob_path(repo.path(), &first.files[0].sha256)
                .unwrap()
                .is_file()
        );
    }

    #[test]
    fn missing_tracked_root_is_empty_while_populated_root_is_captured() {
        let repo = tempdir().unwrap();
        let missing_parent = tempdir().unwrap();
        let populated = tempdir().unwrap();
        fs::write(populated.path().join("save.dat"), b"populated").unwrap();

        let missing = tracked_dir(
            &missing_parent.path().join("not-created"),
            Uuid::from_u128(1),
            "[]",
        );
        let populated = tracked_dir(populated.path(), Uuid::from_u128(2), "[]");

        let snapshot = capture_snapshot(repo.path(), &[missing, populated]).unwrap();

        assert_eq!(snapshot.files.len(), 1);
        assert_eq!(snapshot.files[0].tracked_dir_uuid, Uuid::from_u128(2));
        assert_eq!(snapshot.files[0].relative_path, "save.dat");
    }

    #[test]
    fn content_hash_does_not_depend_on_input_directory_order() {
        let repo = tempdir().unwrap();
        let (a_dir, a) = fixture_tracked_dir("a.dat", b"a", Uuid::from_u128(1));
        let (b_dir, b) = fixture_tracked_dir("b.dat", b"b", Uuid::from_u128(2));
        let forward = capture_snapshot(repo.path(), &[a.clone(), b.clone()]).unwrap();
        let reverse = capture_snapshot(repo.path(), &[b, a]).unwrap();
        let _keep_alive = (a_dir, b_dir);
        assert_eq!(forward.content_hash, reverse.content_hash);
        assert_eq!(forward.files, reverse.files);
    }

    #[test]
    fn concurrent_writers_never_replace_existing_blob() {
        let repo = tempdir().unwrap();
        let hash = format_hash(&Sha256::digest(b"new bytes"));
        let destination = blob_path(repo.path(), &hash).unwrap();
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::write(&destination, b"original bytes").unwrap();
        let first_temp = destination.with_file_name(".first.tmp");
        let second_temp = destination.with_file_name(".second.tmp");
        fs::write(&first_temp, b"new bytes").unwrap();
        fs::write(&second_temp, b"new bytes").unwrap();

        let (first, second) = std::thread::scope(|scope| {
            let first =
                scope.spawn(|| publish_blob_no_replace(&first_temp, &destination, &hash, 9));
            let second =
                scope.spawn(|| publish_blob_no_replace(&second_temp, &destination, &hash, 9));
            (first.join().unwrap(), second.join().unwrap())
        });

        assert!(first.is_err());
        assert!(second.is_err());
        assert_eq!(fs::read(&destination).unwrap(), b"original bytes");
    }

    #[test]
    fn atomic_publication_renames_same_directory_temp_file() {
        let repo = tempdir().unwrap();
        let temp = repo.path().join(".blob.tmp");
        let destination = repo.path().join("blob");
        fs::write(&temp, b"complete bytes").unwrap();

        atomic_rename_no_replace(&temp, &destination).unwrap();

        assert!(!temp.exists());
        assert_eq!(fs::read(destination).unwrap(), b"complete bytes");
    }
}
