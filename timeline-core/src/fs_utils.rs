use anyhow::{Context, Result, anyhow};
use glob::Pattern;
use std::path::{Path, PathBuf};

/// Global exclusions: this is a pure name test with no knowledge of which game or
/// directory it is filtering for, so it may only encode what is *never* save data.
/// Anything game-specific (`.ini`, for instance, which some games use for the save
/// itself) belongs in the per-tracked-dir user exclusions, not here.
pub fn is_excluded_file(name: &str) -> bool {
    let name_lower = name.to_lowercase();
    if name_lower == "steam_autocloud.vdf"
        || name_lower == "remotecache.vdf"
        || name_lower == "output_log.txt"
    {
        return true;
    }
    name_lower.ends_with(".log")
        || name_lower.ends_with(".tmp")
        || name_lower.ends_with(".bak")
        || name_lower.ends_with(".backup")
        || name_lower.ends_with(".cache")
}

/// Copies `src` into `dst`, skipping globally excluded files and anything matching
/// `exclusions` (evaluated against paths relative to `src`).
pub fn copy_dir_filtered(src: &Path, dst: &Path, exclusions: &[Pattern]) -> Result<()> {
    copy_dir_inner(src, dst, exclusions, Path::new(""))
}

fn copy_dir_inner(src: &Path, dst: &Path, exclusions: &[Pattern], relative: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_name = entry.file_name();
        if is_excluded_file(&file_name.to_string_lossy()) {
            continue;
        }
        let entry_relative = relative.join(&file_name);
        if crate::exclusions::is_excluded(exclusions, &entry_relative) {
            continue;
        }
        let path = entry.path();
        let dest_path = dst.join(&file_name);
        if path.is_dir() {
            copy_dir_inner(&path, &dest_path, exclusions, &entry_relative)?;
        } else {
            std::fs::copy(&path, &dest_path)?;
        }
    }
    Ok(())
}

/// Removes everything inside `dir` except entries matching `exclusions`.
/// Directories that still hold excluded entries are kept; empty ones are removed.
///
/// This is the counterpart to `copy_dir_filtered`: a file the user excluded from
/// capture must not be destroyed by a wipe, or the exclusion would lose the very
/// data it was meant to protect.
pub fn clear_dir_except(dir: &Path, exclusions: &[Pattern]) -> Result<()> {
    validate_restore_path(dir, Path::new(""))?;
    clear_dir_inner(dir, exclusions, Path::new(""))?;
    Ok(())
}

/// Validates the existing directory chain from the filesystem root through the
/// parent of a restore destination. Missing tail components are safe because
/// restore creates them only after every tracked directory passes this check.
pub fn validate_restore_path(root: &Path, relative_path: &Path) -> Result<()> {
    let path = if relative_path.as_os_str().is_empty() {
        root.to_path_buf()
    } else {
        root.join(relative_path)
            .parent()
            .unwrap_or(root)
            .to_path_buf()
    };
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        let metadata = match std::fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("failed to inspect restore path {}", current.display())
                });
            }
        };
        if is_link_or_reparse(&metadata) {
            return Err(anyhow!(
                "restore path contains a link or reparse point: {}",
                current.display()
            ));
        }
        if !metadata.is_dir() {
            return Err(anyhow!(
                "restore path ancestor is not a directory: {}",
                current.display()
            ));
        }
    }
    Ok(())
}

/// Returns whether anything was kept inside `dir`.
///
/// Deliberately checks ONLY the user exclusions, unlike its visual sibling
/// `copy_dir_inner`, which also consults the global `is_excluded_file` list. Do not
/// "fix" this for symmetry: globally excluded files (`.log`, `.tmp`, ...) are never
/// captured into a snapshot, so preserving them here would mean they are never
/// refreshed either — they would pile up in the live directory forever, across every
/// slot switch and restore, with no way for the user to clear them. Anything a user
/// actually wants preserved is expressible as a per-directory user exclusion.
fn clear_dir_inner(dir: &Path, exclusions: &[Pattern], relative: &Path) -> Result<bool> {
    let mut kept_anything = false;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let entry_relative = relative.join(entry.file_name());
        if crate::exclusions::is_excluded(exclusions, &entry_relative) {
            kept_anything = true;
            continue;
        }
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path)
            .with_context(|| format!("failed to inspect wipe entry {}", path.display()))?;
        if is_link_or_reparse(&metadata) {
            remove_link_or_reparse(&path)?;
        } else if metadata.is_dir() {
            if clear_dir_inner(&path, exclusions, &entry_relative)? {
                kept_anything = true;
            } else {
                std::fs::remove_dir(&path)?;
            }
        } else {
            std::fs::remove_file(&path)?;
        }
    }
    Ok(kept_anything)
}

fn is_link_or_reparse(metadata: &std::fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes()
            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
            != 0
    }

    #[cfg(not(windows))]
    {
        false
    }
}

fn remove_link_or_reparse(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(file_error) => std::fs::remove_dir(path).map_err(|dir_error| {
            anyhow!(
                "failed to remove link or reparse point {} as a file ({file_error}) or directory ({dir_error})",
                path.display()
            )
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn create_directory_link(target: &Path, link: &Path) {
        std::os::unix::fs::symlink(target, link).unwrap();
    }

    #[cfg(windows)]
    fn create_directory_link(target: &Path, link: &Path) {
        let output = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "failed to create test junction: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn ini_and_multi_part_bak_names_are_not_globally_excluded() {
        // `.contains(".bak")` used to swallow real save files.
        assert!(!is_excluded_file("save.bak2.dat"));
        assert!(!is_excluded_file("config.ini"));
        assert!(is_excluded_file("save.bak"));
        assert!(is_excluded_file("debug.LOG"));
    }

    #[test]
    fn clear_dir_except_keeps_excluded_entries_and_their_parents() {
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(root.path().join("empty")).unwrap();
        std::fs::write(root.path().join("save.dat"), "x").unwrap();
        std::fs::write(nested.join("keep.cfg"), "keep").unwrap();
        std::fs::write(nested.join("drop.dat"), "drop").unwrap();

        clear_dir_except(root.path(), &crate::exclusions::parse(r#"["*.cfg"]"#)).unwrap();

        assert!(!root.path().join("save.dat").exists());
        assert!(!root.path().join("empty").exists());
        assert!(!nested.join("drop.dat").exists());
        assert_eq!(
            std::fs::read_to_string(nested.join("keep.cfg")).unwrap(),
            "keep"
        );
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn clear_dir_except_removes_directory_link_without_following_target() {
        let root = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        let external_file = external.path().join("progress.dat");
        std::fs::write(&external_file, b"external progress").unwrap();
        let link = root.path().join("linked");
        create_directory_link(external.path(), &link);

        clear_dir_except(root.path(), &[]).unwrap();

        assert_eq!(std::fs::read(&external_file).unwrap(), b"external progress");
        assert!(std::fs::symlink_metadata(&link).is_err());
    }
}
