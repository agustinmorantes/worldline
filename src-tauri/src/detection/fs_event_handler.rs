use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use notify::EventKind;
use crate::detection::{Notification, NotificationSender};
use anyhow::Result;
use notify::event::ModifyKind;
use notify_debouncer_full::DebouncedEvent;
use crate::models::TrackedDir;

pub fn fs_event_handler(
    events: Vec<DebouncedEvent>,
    notif_tx: NotificationSender,
    dirs: &[TrackedDir],
    working_base: &Path,
) -> Result<()> {
    let first_scan_dirs = dirs.iter()
        .filter(|dir| {
            let working_dir_path = &working_base.join(dir.id.to_string());
            !working_dir_path.exists()
        })
        .cloned()
        .collect::<Vec<TrackedDir>>();

    handle_rescan(&first_scan_dirs, working_base).ok();

    let mut created_files: HashSet<PathBuf> = HashSet::new();
    let mut deleted_files: HashSet<PathBuf> = HashSet::new();
    let mut modified_files: HashSet<PathBuf> = HashSet::new();
    let mut renamed_files: HashSet<(PathBuf, PathBuf)> = HashSet::new();
    let mut modified_games: HashSet<i32> = HashSet::new();
    for ev in events {
        println!("event detected: {:?}", ev);

        for path in &ev.paths {
            for dir in dirs {
                if path.starts_with(&dir.path) {
                    modified_games.insert(dir.game_id);
                }
            }
        }

        if ev.need_rescan() {
            return handle_rescan(dirs, working_base);
        }

        match ev.kind {
            EventKind::Create(_) => {
                if let Some(path) = ev.paths.first() {
                    created_files.insert(path.clone());
                }
            }
            EventKind::Remove(_) => {
                if let Some(path) = ev.paths.first() {
                    deleted_files.insert(path.clone());
                }
            }
            EventKind::Modify(kind) => match kind {
                ModifyKind::Name(_) => {
                    if let Some(from) = ev.paths.first() {
                       if let Some(to) = ev.paths.last() {
                           renamed_files.insert((from.clone(), to.clone()));
                       }
                    }
                }
                _ => {
                    if let Some(path) = ev.paths.first() {
                        modified_files.insert(path.clone());
                    }
                }
            }
            _ => {} // ignore other events
        }
    }

    handle_deleted_files(&deleted_files, dirs, working_base);
    handle_created_files(&created_files, dirs, working_base);
    handle_modified_files(&modified_files, dirs, working_base);

    for game_id in &modified_games {
        // Send notification about the change
        notif_tx.send(Notification::ChangeDetected {
            game_id: *game_id,
        })?;
    }

    Ok(())
}


fn handle_created_files(
    paths: impl IntoIterator<Item = impl AsRef<Path>>,
    tracked_dirs: &[TrackedDir],
    working_base: &Path,
) {
    handle_modified_files(paths, tracked_dirs, working_base);
}

fn handle_deleted_files(
    paths: impl IntoIterator<Item = impl AsRef<Path>>,
    tracked_dirs: &[TrackedDir],
    working_base: &Path,
) {
    for path in paths {
        let path = path.as_ref();
        let Some(tracked_dir) = tracked_dirs.iter()
            .find(|dir| path.starts_with(&dir.path))
            else {
                continue;
            };

        let tracked_path = Path::new(&tracked_dir.path);
        let rel_path = path.strip_prefix(tracked_path).unwrap_or(path);
        let final_path = &working_base.join(tracked_dir.id.to_string()).join(rel_path);

        // Delete the file if it exists
        if final_path.exists() {
            if let Err(e) = fs::remove_file(final_path) {
                eprintln!("Error deleting file {:?}: {}", final_path, e);
            }
        }
    }
}

fn handle_modified_files(
    paths: impl IntoIterator<Item = impl AsRef<Path>>,
    tracked_dirs: &[TrackedDir],
    working_base: &Path,
) {
    for path in paths {
        let path = path.as_ref();
        let Some(tracked_dir) = tracked_dirs.iter()
            .find(|dir| path.starts_with(&dir.path))
        else {
            continue;
        };

        let tracked_path = Path::new(&tracked_dir.path);
        let rel_path = path.strip_prefix(tracked_path).unwrap_or(path);
        let final_path = &working_base.join(tracked_dir.id.to_string()).join(rel_path);

        // Create parent directories if they don't exist
        if let Some(parent) = final_path.parent() {
            if !parent.exists() {
                if let Err(e) = fs::create_dir_all(parent) {
                    eprintln!("Error creating directories for {:?}: {}", parent, e);
                    continue;
                }
            }
        }

        // Retry if there are errors copying file
        let mut retries = 0;
        loop {
            if retries >= 5 {
                eprintln!("Failed to copy file {:?} to {:?} after {} retries. Skipping.", path, final_path, retries);
                break;
            }
            match fs::copy(path, final_path) {
                Ok(_) => break,
                Err(e) => {
                    eprintln!("Error copying file {:?} to {:?}: {}. Retrying...", path, final_path, e);
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    retries += 1;
                }
            }
        }
    }
}

fn handle_rescan(dirs: &[TrackedDir], working_base: &Path) -> Result<()> {
    for dir in dirs {
        let path = Path::new(&dir.path);
        let working_dir_path = &working_base.join(dir.id.to_string());

        if working_dir_path.exists() {
            fs::remove_dir_all(working_dir_path)?;
        }
        else if !working_dir_path.parent().unwrap().exists() {
            fs::create_dir_all(working_dir_path.parent().unwrap())?;
        }

        let mut options = fs_extra::dir::CopyOptions::new();
        options.copy_inside = true;
        options.overwrite = true;
        fs_extra::dir::copy(path, working_dir_path, &options)?;
    }

    Ok(())
}
