//! Which games are running: a game runs while some process's executable lives
//! under its install dir. A thread enumerates processes every few seconds,
//! only while some game has an install dir that exists, and reports each
//! start and stop, so front ends never poll.

use super::Notification;
use anyhow::Result;
use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

const INTERVAL: Duration = Duration::from_secs(3);

/// The install dir of every game that has one, by game id.
pub type InstallDirs = Box<dyn Fn() -> Result<Vec<(i32, String)>> + Send>;

pub struct RunningMonitor {
    stop: mpsc::Sender<()>,
    thread: Option<JoinHandle<()>>,
    running: Arc<Mutex<HashSet<i32>>>,
}

impl RunningMonitor {
    pub fn start(installs: InstallDirs, notifications: mpsc::Sender<Notification>) -> Self {
        let (stop, stopped) = mpsc::channel();
        let running = Arc::new(Mutex::new(HashSet::new()));
        let shared = running.clone();
        let thread = std::thread::spawn(move || run(&installs, &notifications, &shared, &stopped));
        Self {
            stop,
            thread: Some(thread),
            running,
        }
    }

    /// The games running as of the last check.
    pub fn running(&self) -> Vec<i32> {
        let mut ids: Vec<i32> = self
            .running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .copied()
            .collect();
        ids.sort_unstable();
        ids
    }
}

impl Drop for RunningMonitor {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(
    installs: &InstallDirs,
    notifications: &mpsc::Sender<Notification>,
    running: &Mutex<HashSet<i32>>,
    stopped: &mpsc::Receiver<()>,
) {
    let mut system = sysinfo::System::new();
    loop {
        // An unreadable index or process list keeps the last known state.
        if let Ok(dirs) = installs() {
            let dirs: Vec<(i32, String)> = dirs
                .into_iter()
                .filter(|(_, dir)| Path::new(dir).is_dir())
                .collect();
            let now = if dirs.is_empty() {
                Some(HashSet::new())
            } else {
                system.refresh_processes_specifics(
                    sysinfo::ProcessesToUpdate::All,
                    true,
                    sysinfo::ProcessRefreshKind::nothing()
                        .with_exe(sysinfo::UpdateKind::OnlyIfNotSet),
                );
                let executables: Vec<&Path> = system
                    .processes()
                    .values()
                    .filter_map(|p| p.exe())
                    .collect();
                (!executables.is_empty()).then(|| running_games(&dirs, &executables))
            };
            if let Some(now) = now {
                let mut running = running.lock().unwrap_or_else(|e| e.into_inner());
                for &game_id in now.difference(&running) {
                    let _ = notifications.send(Notification::GameStarted { game_id });
                }
                for &game_id in running.difference(&now) {
                    let _ = notifications.send(Notification::GameStopped { game_id });
                }
                *running = now;
            }
        }
        match stopped.recv_timeout(INTERVAL) {
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            _ => return,
        }
    }
}

/// The games with an executable under their install dir.
fn running_games(dirs: &[(i32, String)], executables: &[&Path]) -> HashSet<i32> {
    dirs.iter()
        .filter(|(_, dir)| executables.iter().any(|exe| runs_from(exe, dir)))
        .map(|(id, _)| *id)
        .collect()
}

/// The processes whose executable lives under `dir`.
pub(crate) fn pids_under(dir: &str) -> Vec<u32> {
    let mut system = sysinfo::System::new();
    system.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::All,
        true,
        sysinfo::ProcessRefreshKind::nothing().with_exe(sysinfo::UpdateKind::OnlyIfNotSet),
    );
    system
        .processes()
        .iter()
        .filter(|(_, p)| p.exe().is_some_and(|exe| runs_from(exe, dir)))
        .map(|(pid, _)| pid.as_u32())
        .collect()
}

/// Whether `exe` lives under `dir`; case-insensitive on Windows.
pub(crate) fn runs_from(exe: &Path, dir: &str) -> bool {
    if cfg!(windows) {
        let exe = exe.to_string_lossy().replace('/', "\\").to_lowercase();
        let dir = dir.replace('/', "\\").to_lowercase();
        exe.starts_with(&format!("{}\\", dir.trim_end_matches('\\')))
    } else {
        exe.starts_with(dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_game_runs_while_an_executable_lives_under_its_install_dir() {
        let (game, sibling) = if cfg!(windows) {
            (r"C:\Games\Hollow", r"C:\Games\Hollow Knight")
        } else {
            ("/games/Hollow", "/games/Hollow Knight")
        };
        let dirs = [(1, game.to_string()), (2, sibling.to_string())];
        let exe = if cfg!(windows) {
            Path::new(r"c:/games/hollow/bin/Game.exe")
        } else {
            Path::new("/games/Hollow/bin/game")
        };
        assert_eq!(running_games(&dirs, &[exe]), HashSet::from([1]));
        let other = if cfg!(windows) {
            Path::new(r"C:\Games\Hollow.exe")
        } else {
            Path::new("/games/Hollow.sh")
        };
        assert!(running_games(&dirs, &[other]).is_empty());
    }
}
