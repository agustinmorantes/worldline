use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use std::time::Duration;
use tempfile::tempdir;

fn cli(data_dir: &std::path::Path) -> Command {
    let mut cmd = Command::cargo_bin("cli").unwrap();
    cmd.arg("--data-dir").arg(data_dir);
    cmd
}

/// Start a `watch` subprocess in the background (0s debounce) and return the handle.
/// The caller must kill + wait on the handle when done.
fn start_watch(data_dir: &std::path::Path) -> std::process::Child {
    std::process::Command::new(cargo_bin("cli"))
        .arg("--data-dir")
        .arg(data_dir)
        .arg("--debounce-secs")
        .arg("0")
        .arg("watch")
        .spawn()
        .expect("failed to spawn watch process")
}

/// Add a game with a tracked dir, trigger a file change via the background watcher,
/// and return (save_dir, save_file). The caller is responsible for the data_dir lifetime.
fn add_game_and_trigger_version(
    data_dir: &std::path::Path,
    title: &str,
) -> (std::path::PathBuf, std::path::PathBuf) {
    let save_dir = tempdir().unwrap().keep();
    let save_file = save_dir.join("save.dat");
    std::fs::write(&save_file, "initial save").unwrap();

    cli(data_dir)
        .args(["games", "add", title, "--dir", save_dir.to_str().unwrap()])
        .assert()
        .success();

    let mut watch_proc = start_watch(data_dir);
    std::thread::sleep(Duration::from_millis(200));

    std::fs::write(&save_file, "after change").unwrap();
    std::thread::sleep(Duration::from_millis(2500));

    watch_proc.kill().ok();
    watch_proc.wait().ok();

    (save_dir, save_file)
}

// ── Games ────────────────────────────────────────────────────────────────────

#[test]
fn games_list_empty() {
    let dir = tempdir().unwrap();
    cli(dir.path())
        .args(["games", "list"])
        .assert()
        .success()
        .stdout("No games.\n");
}

#[test]
fn games_add_and_list() {
    let dir = tempdir().unwrap();
    cli(dir.path())
        .args(["games", "add", "Hollow Knight"])
        .assert()
        .success()
        .stdout(contains("Hollow Knight"));

    cli(dir.path())
        .args(["games", "list"])
        .assert()
        .success()
        .stdout(contains("Hollow Knight"));
}

#[test]
fn games_add_with_dirs() {
    let dir = tempdir().unwrap();
    let save_dir = tempdir().unwrap();
    let save_path = save_dir.path().to_str().unwrap();

    cli(dir.path())
        .args(["games", "add", "Dark Souls", "--dir", save_path])
        .assert()
        .success();

    cli(dir.path())
        .args(["games", "show", "1"])
        .assert()
        .success()
        .stdout(contains(save_path));
}

#[test]
fn games_show() {
    let dir = tempdir().unwrap();
    cli(dir.path())
        .args(["games", "add", "Celeste"])
        .assert()
        .success();

    cli(dir.path())
        .args(["games", "show", "1"])
        .assert()
        .success()
        .stdout(contains("Celeste"))
        .stdout(contains("Tracked dirs"))
        .stdout(contains("Versions (0)"));
}

#[test]
fn games_delete() {
    let dir = tempdir().unwrap();
    cli(dir.path())
        .args(["games", "add", "Delete Me"])
        .assert()
        .success();

    cli(dir.path())
        .args(["games", "delete", "1"])
        .assert()
        .success()
        .stdout(contains("Deleted game 1"));

    cli(dir.path())
        .args(["games", "list"])
        .assert()
        .success()
        .stdout("No games.\n");
}

#[test]
fn games_edit() {
    let dir = tempdir().unwrap();
    cli(dir.path())
        .args(["games", "add", "Old Title"])
        .assert()
        .success();

    cli(dir.path())
        .args(["games", "edit", "1", "New Title"])
        .assert()
        .success()
        .stdout(contains("New Title"));

    cli(dir.path())
        .args(["games", "list"])
        .assert()
        .success()
        .stdout(contains("New Title"))
        .stdout(predicates::str::contains("Old Title").not());
}

// ── Versions ─────────────────────────────────────────────────────────────────

#[test]
fn version_created_after_file_change() {
    let dir = tempdir().unwrap();
    add_game_and_trigger_version(dir.path(), "Watcher Game");

    let output = cli(dir.path())
        .args(["games", "show", "1"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let text = String::from_utf8_lossy(&output);
    assert!(
        text.contains("Versions (1)") || text.contains("Versions (2)"),
        "expected at least one version, got: {text}"
    );
}

#[test]
fn versions_tag() {
    let dir = tempdir().unwrap();
    add_game_and_trigger_version(dir.path(), "Tag Game");

    // Get the version id from show output (first version listed as [N])
    let output = cli(dir.path())
        .args(["games", "show", "1"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8_lossy(&output);
    let version_id = parse_first_version_id(&text).expect("no version found");

    cli(dir.path())
        .args(["versions", "tag", &version_id.to_string(), "before-boss"])
        .assert()
        .success()
        .stdout(contains("before-boss"));

    cli(dir.path())
        .args(["games", "show", "1"])
        .assert()
        .success()
        .stdout(contains("before-boss"));
}

#[test]
fn versions_restore() {
    let dir = tempdir().unwrap();
    let (_, save_file) = add_game_and_trigger_version(dir.path(), "Restore Game");

    let output = cli(dir.path())
        .args(["games", "show", "1"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8_lossy(&output);
    let version_id = parse_first_version_id(&text).expect("no version found");

    std::fs::write(&save_file, "corrupted!").unwrap();

    cli(dir.path())
        .args(["versions", "restore", &version_id.to_string()])
        .assert()
        .success()
        .stdout(contains(format!("Restored version {version_id}")));

    let content = std::fs::read_to_string(&save_file).unwrap();
    assert_eq!(content, "after change");
}

#[test]
fn versions_restore_nonexistent_fails() {
    let dir = tempdir().unwrap();
    cli(dir.path())
        .args(["versions", "restore", "9999"])
        .assert()
        .failure();
}

// ── Slots ────────────────────────────────────────────────────────────────────

#[test]
fn slots_list_shows_the_default_slot() {
    let dir = tempdir().unwrap();
    cli(dir.path())
        .args(["games", "add", "Slot CLI Game"])
        .assert()
        .success();

    cli(dir.path())
        .args(["slots", "list", "1"])
        .assert()
        .success()
        .stdout(contains("Default Slot"));
}

#[test]
fn slots_create_adds_a_new_slot() {
    let dir = tempdir().unwrap();
    cli(dir.path())
        .args(["games", "add", "Create Slot Game"])
        .assert()
        .success();

    cli(dir.path())
        .args(["slots", "create", "1", "New Game Plus"])
        .assert()
        .success()
        .stdout(contains("New Game Plus"));

    cli(dir.path())
        .args(["slots", "list", "1"])
        .assert()
        .success()
        .stdout(contains("Default Slot"))
        .stdout(contains("New Game Plus"));
}

#[test]
fn slots_switch_changes_the_active_slot() {
    let dir = tempdir().unwrap();
    cli(dir.path())
        .args(["games", "add", "Switch Slot Game"])
        .assert()
        .success();
    // create_slot activates the new slot, so slot 2 (New Game Plus) starts active.
    cli(dir.path())
        .args(["slots", "create", "1", "New Game Plus"])
        .assert()
        .success();

    cli(dir.path())
        .args(["slots", "switch", "1", "1"])
        .assert()
        .success()
        .stdout(contains("Default Slot"));

    cli(dir.path())
        .args(["slots", "list", "1"])
        .assert()
        .success()
        .stdout(contains("* [1] Default Slot"));
}

#[test]
fn slots_rename_changes_the_name() {
    let dir = tempdir().unwrap();
    cli(dir.path())
        .args(["games", "add", "Rename Slot Game"])
        .assert()
        .success();

    cli(dir.path())
        .args(["slots", "rename", "1", "Renamed Slot"])
        .assert()
        .success()
        .stdout(contains("Renamed Slot"));

    cli(dir.path())
        .args(["slots", "list", "1"])
        .assert()
        .success()
        .stdout(contains("Renamed Slot"))
        .stdout(contains("Default Slot").not());
}

#[test]
fn slots_rename_to_duplicate_name_fails_with_message() {
    let dir = tempdir().unwrap();
    cli(dir.path())
        .args(["games", "add", "Dup Rename Game"])
        .assert()
        .success();
    cli(dir.path())
        .args(["slots", "create", "1", "Second Slot"])
        .assert()
        .success();

    cli(dir.path())
        .args(["slots", "rename", "1", "Second Slot"])
        .assert()
        .failure()
        .stderr(contains("already exists"));
}

#[test]
fn slots_delete_last_slot_fails_with_message() {
    let dir = tempdir().unwrap();
    cli(dir.path())
        .args(["games", "add", "Last Slot Game"])
        .assert()
        .success();

    cli(dir.path())
        .args(["slots", "delete", "1"])
        .assert()
        .failure()
        .stderr(contains("must keep at least one slot"));
}

#[test]
fn slots_delete_active_slot_without_replacement_fails_with_message() {
    let dir = tempdir().unwrap();
    cli(dir.path())
        .args(["games", "add", "Active Delete Game"])
        .assert()
        .success();
    // create_slot activates slot 2, so it is now the active slot.
    cli(dir.path())
        .args(["slots", "create", "1", "Second Slot"])
        .assert()
        .success();

    cli(dir.path())
        .args(["slots", "delete", "2"])
        .assert()
        .failure()
        .stderr(contains("requires a replacement slot"));
}

#[test]
fn slots_delete_with_replacement_succeeds() {
    let dir = tempdir().unwrap();
    cli(dir.path())
        .args(["games", "add", "Delete With Replacement Game"])
        .assert()
        .success();
    cli(dir.path())
        .args(["slots", "create", "1", "Second Slot"])
        .assert()
        .success();

    // Slot 2 is active; replace it with slot 1 before deleting it.
    cli(dir.path())
        .args(["slots", "delete", "2", "--replacement", "1"])
        .assert()
        .success()
        .stdout(contains("Deleted slot 2"));

    cli(dir.path())
        .args(["slots", "list", "1"])
        .assert()
        .success()
        .stdout(contains("Second Slot").not());
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Parse the first version id from `games show` output.
/// Lines under "Versions" look like: "    [7] #1 at ... tag=..."
fn parse_first_version_id(text: &str) -> Option<i32> {
    let in_versions = text.lines().skip_while(|l| !l.contains("Versions ("));
    for line in in_versions.skip(1) {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix('[')
            && let Some(id_str) = rest.split(']').next()
            && let Ok(id) = id_str.parse::<i32>()
        {
            return Some(id);
        }
    }
    None
}
