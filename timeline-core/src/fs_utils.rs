use anyhow::Result;
use std::path::Path;

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

pub fn copy_dir_filtered(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_name = entry.file_name();
        let file_name_str = file_name.to_string_lossy();
        if is_excluded_file(&file_name_str) {
            continue;
        }
        let path = entry.path();
        let dest_path = dst.join(&file_name);
        if path.is_dir() {
            copy_dir_filtered(&path, &dest_path)?;
        } else {
            std::fs::copy(&path, &dest_path)?;
        }
    }
    Ok(())
}
