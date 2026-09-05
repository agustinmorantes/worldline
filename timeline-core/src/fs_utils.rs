use anyhow::Result;
use glob::Pattern;
use std::path::Path;

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
    clear_dir_inner(dir, exclusions, Path::new(""))?;
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
        if path.is_dir() {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
