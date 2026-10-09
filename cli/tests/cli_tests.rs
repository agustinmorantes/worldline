use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin;
use predicates::str::contains;
use std::fs;
use std::path::Path;
use std::time::Duration;
use tempfile::tempdir;

fn cli(data_dir: &Path) -> Command {
    let mut cmd = Command::cargo_bin("cli").unwrap();
    cmd.arg("--data-dir").arg(data_dir);
    cmd
}

fn stdout(data_dir: &Path, args: &[&str]) -> String {
    let output = cli(data_dir)
        .args(args)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8(output).unwrap()
}

/// The first `[id]` on a line that contains `needle`.
fn id_on_line(text: &str, needle: &str) -> i32 {
    let line = text
        .lines()
        .find(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("{needle:?} not in {text}"));
    let start = line.find('[').unwrap() + 1;
    let end = line[start..].find(']').unwrap() + start;
    line[start..end].parse().unwrap()
}

fn add_game(data_dir: &Path, title: &str, save_dir: &Path) -> i32 {
    let out = stdout(
        data_dir,
        &["games", "add", title, "--dir", save_dir.to_str().unwrap()],
    );
    id_on_line(&out, "Added game")
}

#[test]
fn games_crud() {
    let dir = tempdir().unwrap();
    cli(dir.path())
        .args(["games", "list"])
        .assert()
        .success()
        .stdout(contains("No games."));
    let saves = tempdir().unwrap();
    fs::write(saves.path().join("save.dat"), "x").unwrap();
    let id = add_game(dir.path(), "Test Game", saves.path());
    cli(dir.path())
        .args(["games", "list"])
        .assert()
        .success()
        .stdout(contains("Test Game — 1 version(s), 1 tracked dir(s)"));
    cli(dir.path())
        .args(["games", "edit", &id.to_string(), "Renamed"])
        .assert()
        .success();
    cli(dir.path())
        .args(["games", "show", &id.to_string()])
        .assert()
        .success()
        .stdout(contains("Renamed"));
    cli(dir.path())
        .args(["games", "add", "renamed"])
        .assert()
        .failure()
        .stderr(contains("already exists"));
    cli(dir.path())
        .args(["games", "delete", &id.to_string()])
        .assert()
        .success();
    cli(dir.path())
        .args(["games", "list"])
        .assert()
        .success()
        .stdout(contains("No games."));
}

#[test]
fn versions_checkpoint_tag_and_restore() {
    let dir = tempdir().unwrap();
    let saves = tempdir().unwrap();
    let save = saves.path().join("save.dat");
    fs::write(&save, "first").unwrap();
    let id = add_game(dir.path(), "Versions", saves.path());
    cli(dir.path())
        .args(["versions", "checkpoint", "Versions"])
        .assert()
        .success()
        .stdout(contains("Unchanged"));
    fs::write(&save, "second").unwrap();
    cli(dir.path())
        .args(["versions", "checkpoint", "Versions"])
        .assert()
        .success()
        .stdout(contains("Created version"));
    let show = stdout(dir.path(), &["games", "show", &id.to_string()]);
    let first = id_on_line(&show, "#1 ");
    cli(dir.path())
        .args(["versions", "tag", &first.to_string(), "start"])
        .assert()
        .success()
        .stdout(contains("\"start\""));
    cli(dir.path())
        .args(["versions", "restore", &first.to_string()])
        .assert()
        .success();
    assert_eq!(fs::read_to_string(&save).unwrap(), "first");
    cli(dir.path())
        .args(["versions", "restore", "9999"])
        .assert()
        .failure();
}

#[test]
fn slots_lifecycle() {
    let dir = tempdir().unwrap();
    let saves = tempdir().unwrap();
    fs::write(saves.path().join("save.dat"), "main").unwrap();
    add_game(dir.path(), "Slots", saves.path());
    cli(dir.path())
        .args(["slots", "list", "Slots"])
        .assert()
        .success()
        .stdout(contains("* ["));
    cli(dir.path())
        .args(["slots", "create", "Slots", "Speedrun"])
        .assert()
        .success()
        .stdout(contains("Created slot"));
    assert!(!saves.path().join("save.dat").exists());
    cli(dir.path())
        .args(["slots", "switch", "Slots", "Default Slot"])
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(saves.path().join("save.dat")).unwrap(),
        "main"
    );
    cli(dir.path())
        .args(["slots", "rename", "Slots", "Speedrun", "default slot"])
        .assert()
        .failure()
        .stderr(contains("already exists"));
    cli(dir.path())
        .args(["slots", "delete", "Slots", "Default Slot"])
        .assert()
        .failure()
        .stderr(contains("replacement"));
    cli(dir.path())
        .args([
            "slots",
            "delete",
            "Slots",
            "Default Slot",
            "--replacement",
            "Speedrun",
        ])
        .assert()
        .success();
    cli(dir.path())
        .args(["slots", "delete", "Slots", "Speedrun"])
        .assert()
        .failure()
        .stderr(contains("at least one slot"));
}

#[test]
fn prune_settings_preview_and_run() {
    let dir = tempdir().unwrap();
    cli(dir.path())
        .args(["prune", "preview"])
        .assert()
        .success()
        .stdout(contains("0 checkpoint(s)"));
    cli(dir.path())
        .args([
            "prune",
            "settings",
            "--max-bytes",
            "1000000",
            "--branch-expiry-days",
            "0",
        ])
        .assert()
        .success()
        .stdout(contains("Automatic: true"))
        .stdout(contains("Disk budget: 1000000"))
        .stdout(contains("never expire"));
    cli(dir.path())
        .args(["prune", "run"])
        .assert()
        .success()
        .stdout(contains("Retired 0"));
}

#[test]
fn repo_check_extract_and_reindex() {
    let dir = tempdir().unwrap();
    let saves = tempdir().unwrap();
    fs::write(saves.path().join("save.dat"), "recover me").unwrap();
    let id = add_game(dir.path(), "Recover", saves.path());
    let version = id_on_line(
        &stdout(dir.path(), &["games", "show", &id.to_string()]),
        "#1 ",
    );
    cli(dir.path())
        .args(["repo", "check", "--deep"])
        .assert()
        .success()
        .stdout(contains("0 missing, 0 corrupt"));
    let out = tempdir().unwrap();
    cli(dir.path())
        .args([
            "repo",
            "extract",
            &version.to_string(),
            out.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(contains("Extracted 1 file(s)"));
    fs::remove_file(dir.path().join("database.db")).unwrap();
    cli(dir.path())
        .args(["games", "list"])
        .assert()
        .success()
        .stdout(contains("Recover"));
    cli(dir.path()).args(["repo", "reindex"]).assert().success();
}

/// Checkpoint → sync between two data dirs → prune → export → import → restore.
#[test]
fn end_to_end_across_two_data_dirs() {
    let (a, b, c) = (tempdir().unwrap(), tempdir().unwrap(), tempdir().unwrap());
    let remote = tempdir().unwrap();
    let saves = tempdir().unwrap();
    let save = saves.path().join("save.dat");
    fs::write(&save, "one").unwrap();
    add_game(a.path(), "Journey", saves.path());
    fs::write(&save, "two").unwrap();
    cli(a.path())
        .args(["versions", "checkpoint", "Journey"])
        .assert()
        .success();

    for data in [a.path(), b.path()] {
        cli(data)
            .args(["sync", "configure", remote.path().to_str().unwrap()])
            .assert()
            .success();
    }
    cli(a.path())
        .args(["devices", "set", "--name", "Deck", "--kind", "handheld"])
        .assert()
        .success()
        .stdout(contains("This device: Deck (handheld)"));
    cli(a.path()).arg("sync").assert().success();
    let status = stdout(b.path(), &["sync", "status"]);
    assert!(
        status.contains("Journey")
            && status.contains("Not tracked here; 2 version(s), last played on Deck"),
        "{status}"
    );
    let uuid = status.lines().find(|l| l.contains("Journey")).unwrap();
    let uuid = &uuid[uuid.find('(').unwrap() + 1..uuid.find(')').unwrap()];
    cli(b.path())
        .args(["sync", "adopt", uuid])
        .assert()
        .success()
        .stdout(contains("2 version(s)"));
    cli(b.path()).arg("sync").assert().success();
    cli(b.path())
        .args(["devices", "list"])
        .assert()
        .success()
        .stdout(contains("Deck ("))
        .stdout(contains("), handheld — last seen"));

    cli(a.path())
        .args([
            "prune",
            "settings",
            "--keep-all-hours",
            "0",
            "--hourly-until-hours",
            "0",
            "--daily-until-days",
            "0",
            "--weekly-until-days",
            "0",
        ])
        .assert()
        .success();
    cli(a.path())
        .args(["prune", "run"])
        .assert()
        .success()
        .stdout(contains("Remote:"));

    let bundle = c.path().join("journey-bundle.zip");
    cli(b.path())
        .args(["history", "export", "Journey", bundle.to_str().unwrap()])
        .assert()
        .success()
        .stdout(contains("Exported Journey"));
    let inspect = stdout(c.path(), &["history", "inspect", bundle.to_str().unwrap()]);
    let dir_uuid = inspect
        .lines()
        .find(|l| l.contains("source hint"))
        .unwrap()
        .trim();
    let dir_uuid = &dir_uuid[..36];
    let restored = tempdir().unwrap();
    let map = format!("{dir_uuid}={}", restored.path().display());
    cli(c.path())
        .args(["history", "import", bundle.to_str().unwrap(), "--map", &map])
        .assert()
        .success()
        .stdout(contains("Imported Journey"));
    let show = stdout(c.path(), &["games", "show", "1"]);
    let first = id_on_line(&show, "#1 ");
    cli(c.path())
        .args(["versions", "restore", &first.to_string()])
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(restored.path().join("save.dat")).unwrap(),
        "one"
    );
}

#[test]
fn untrack_keeps_the_history_in_the_sync_folder_for_adoption() {
    let (data, remote, saves) = (tempdir().unwrap(), tempdir().unwrap(), tempdir().unwrap());
    fs::write(saves.path().join("save.dat"), "one").unwrap();
    let id = add_game(data.path(), "Paused", saves.path()).to_string();
    cli(data.path())
        .args(["games", "untrack", &id])
        .assert()
        .failure()
        .stderr(contains("when sync is on"));

    cli(data.path())
        .args(["sync", "configure", remote.path().to_str().unwrap()])
        .assert()
        .success();
    cli(data.path()).arg("sync").assert().success();
    cli(data.path())
        .args(["games", "untrack", &id])
        .assert()
        .success()
        .stdout(contains("Untracked game"));
    cli(data.path())
        .args(["games", "list"])
        .assert()
        .success()
        .stdout(contains("No games."));
    let status = stdout(data.path(), &["sync", "status"]);
    let line = status.lines().find(|l| l.contains("Paused")).unwrap();
    assert!(line.contains("Not tracked here"), "{status}");
    let uuid = &line[line.find('(').unwrap() + 1..line.find(')').unwrap()];
    cli(data.path())
        .args(["sync", "adopt", uuid])
        .assert()
        .success()
        .stdout(contains("1 version(s)"));
}

#[test]
fn push_pull_and_refresh_are_no_longer_subcommands() {
    let data = tempdir().unwrap();
    for name in ["push", "pull", "refresh"] {
        cli(data.path()).args(["sync", name]).assert().failure();
    }
}

#[test]
fn watch_captures_changes() {
    let dir = tempdir().unwrap();
    let saves = tempdir().unwrap();
    fs::write(saves.path().join("save.dat"), "start").unwrap();
    // No cooldown, so the change right after the first version is captured.
    fs::write(
        dir.path().join("monitoring_settings.json"),
        r#"{"version_cooldown_minutes":0,"snapshot_debounce_seconds":0.1}"#,
    )
    .unwrap();
    let id = add_game(dir.path(), "Watched", saves.path());
    struct KillOnDrop(std::process::Child);
    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            self.0.kill().ok();
            self.0.wait().ok();
        }
    }
    let _watch = KillOnDrop(
        std::process::Command::new(cargo_bin("cli"))
            .arg("--data-dir")
            .arg(dir.path())
            .arg("watch")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    let mut captured = false;
    for n in 0..30 {
        fs::write(saves.path().join("save.dat"), format!("change {n}")).unwrap();
        std::thread::sleep(Duration::from_millis(500));
        if stdout(dir.path(), &["games", "show", &id.to_string()]).contains("#2 ") {
            captured = true;
            break;
        }
    }
    assert!(captured, "the watcher never captured a change");
}
