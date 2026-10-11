//! Writing a version's content back into the live tracked directories.
//! Everything is validated (paths, and every chunk decompressed and hashed)
//! before the first live file is touched, and the write is a transaction
//! (ADR 0008): the live folders end as they were or as the version has them.

use crate::chunks;
use crate::error::ErrorKind;
use crate::models::TrackedDir;
use crate::store::{self, FileEntry, GameRecord, Store, VersionMeta};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub struct PreparedRestore {
    /// Live roots to replace (except their exclusions).
    roots: BTreeMap<PathBuf, Vec<glob::Pattern>>,
    /// Each file to write: its root and its path below it.
    files: Vec<(PathBuf, PathBuf, FileEntry)>,
}

/// Prepares restoring `version`. `record` supplies every tracked directory the
/// game ever had (retired ones too); `live_dirs` are the current ones, which
/// are cleared even when the version holds nothing for them.
pub fn prepare(
    store: &Store,
    record: &GameRecord,
    live_dirs: &[TrackedDir],
    version: &VersionMeta,
    retired: bool,
) -> Result<PreparedRestore> {
    if retired {
        bail!("This checkpoint was retired and can no longer be restored");
    }
    let mut known: HashMap<Uuid, (PathBuf, Vec<glob::Pattern>)> = HashMap::new();
    for dir in &record.tracked_dirs {
        let path = crate::path_placeholders::expand_path_placeholders(&dir.path);
        let exclusions = dir
            .exclusions
            .iter()
            .filter_map(|pattern| glob::Pattern::new(pattern).ok())
            .collect();
        known.insert(dir.uuid, (PathBuf::from(path), exclusions));
    }
    let mut prepared = clear_only(live_dirs)?;
    for dir in live_dirs {
        let uuid = Uuid::parse_str(&dir.uuid)?;
        known.insert(
            uuid,
            (
                PathBuf::from(&dir.path),
                crate::exclusions::parse(&dir.exclusions),
            ),
        );
    }
    for file in &version.files {
        let (root, exclusions) = known.get(&file.tracked_dir_uuid).with_context(|| {
            format!(
                "Version references unknown tracked directory {}",
                file.tracked_dir_uuid
            )
        })?;
        prepared
            .roots
            .entry(root.clone())
            .or_insert_with(|| exclusions.clone());
        let relative = Path::new(&file.path);
        if crate::exclusions::is_excluded(exclusions, relative) {
            continue;
        }
        crate::fs_utils::validate_restore_path(root, relative).with_context(|| {
            format!(
                "Unsafe restore destination {}",
                root.join(relative).display()
            )
        })?;
        chunks::verify_file(store, file)
            .with_context(|| format!("Stored copy of {} is damaged", file.path))?;
        prepared
            .files
            .push((root.clone(), relative.to_path_buf(), file.clone()));
    }
    for root in prepared.roots.keys() {
        validate_root(root)?;
    }
    Ok(prepared)
}

/// A restore replaces its roots wholesale, so a root must be a real folder
/// path that no other app shares.
fn validate_root(root: &Path) -> Result<()> {
    if crate::path_placeholders::is_shared_root(root) {
        bail!(
            "Unsafe tracked dir {}: other apps keep files there too",
            root.display()
        );
    }
    crate::fs_utils::validate_restore_path(root, Path::new(""))
        .with_context(|| format!("Unsafe tracked dir {}", root.display()))
}

/// Prepares emptying the live directories (activating a slot with no versions).
pub fn clear_only(live_dirs: &[TrackedDir]) -> Result<PreparedRestore> {
    let mut roots = BTreeMap::new();
    for dir in live_dirs {
        let path = PathBuf::from(&dir.path);
        validate_root(&path)?;
        roots.insert(path, crate::exclusions::parse(&dir.exclusions));
    }
    Ok(PreparedRestore {
        roots,
        files: Vec::new(),
    })
}

/// One transaction's journal, `<txn>/<id>.json`, so a crash or a rollback
/// that couldn't finish is rolled back later, without the user. Its folder `<txn>/<id>/` holds `new/<n>`
/// (the staged files) and `old/<n>` (the live files moved aside), `n` being
/// the root's index.
#[derive(Serialize, Deserialize)]
struct Journal {
    phase: Phase,
    roots: Vec<PathBuf>,
    /// The files being written, by root index and path below it.
    targets: Vec<(usize, PathBuf)>,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
enum Phase {
    /// Live files may be in `old/`; nothing is placed yet.
    MovingAside,
    /// Every live file is in `old/`; targets may be placed.
    Swapping,
    /// Done; only leftovers remain.
    Committed,
}

/// Where a rollback keeps a moved-aside file it found replaced (the game
/// wrote one in its place): `<txn>/kept/<id>/<n>/`. Never deleted by us.
const KEPT: &str = "kept";

/// What `recover` settled.
pub struct Recovery {
    /// Transactions rolled back.
    pub rolled_back: usize,
    /// The roots of transactions whose rollback can't finish yet (a file is
    /// still held); they're retried on the next `recover`.
    pub pending: Vec<PathBuf>,
}

/// Replaces the live roots' content (except exclusions) with `prepared`'s
/// files, all or nothing. The transaction lives in `txn_root`, so nothing
/// but save files ever goes into a game's folders. A live file the game
/// holds fails the move with `SavesInUse`, and everything is put back. A
/// rollback that can't finish fails with `RestoreIncomplete` and stays
/// journaled: `recover` finishes it once the files are free.
pub fn apply(store: &Store, prepared: PreparedRestore, txn_root: &Path) -> Result<()> {
    let (roots, exclusions): (Vec<PathBuf>, Vec<Vec<glob::Pattern>>) =
        prepared.roots.into_iter().unzip();
    let files: Vec<(usize, PathBuf, FileEntry)> = prepared
        .files
        .into_iter()
        .map(|(root, relative, entry)| {
            let n = roots
                .iter()
                .position(|r| *r == root)
                .expect("every file's root is a prepared root");
            (n, relative, entry)
        })
        .collect();
    let dir = txn_root.join(store::short_random());
    let journal_path = dir.with_extension("json");
    let mut journal = Journal {
        phase: Phase::MovingAside,
        roots,
        targets: files
            .iter()
            .map(|(n, path, _)| (*n, path.clone()))
            .collect(),
    };
    if let Err(error) = stage(store, &dir, &files) {
        if crashed() {
            return Err(error);
        }
        let _ = fs::remove_dir_all(&dir);
        return Err(error.context("Nothing was changed"));
    }
    let mut placed = 0;
    let moved = match swap(&dir, &journal_path, &mut journal, &exclusions, &mut placed) {
        Ok(moved) => moved,
        Err(error) => {
            if crashed() {
                return Err(error);
            }
            if roll_back(&dir, &journal, &journal.targets[..placed]) {
                let _ = fs::remove_dir_all(&dir);
                let _ = fs::remove_file(&journal_path);
                return Err(error.context("Nothing was changed"));
            }
            return Err(error
                .context(format!(
                    "Couldn't put every save back yet; the rest wait in {}",
                    dir.join("old").display()
                ))
                .context(ErrorKind::RestoreIncomplete));
        }
    };
    // Committed: what's left is cleanup, which can't fail the restore.
    for (n, path) in &moved {
        let root = &journal.roots[*n];
        prune_empty(root, &root.join(path));
    }
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_file(&journal_path);
    Ok(())
}

/// Writes every target file into `<dir>/new/<n>`. Touches no live file.
fn stage(store: &Store, dir: &Path, files: &[(usize, PathBuf, FileEntry)]) -> Result<()> {
    for (n, relative, entry) in files {
        let path = dir.join("new").join(n.to_string()).join(relative);
        let parent = path.parent().expect("a staged file has a parent");
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
        failpoint()?;
        let mut file =
            File::create(&path).with_context(|| format!("Failed to create {}", path.display()))?;
        chunks::write_file(store, entry, &mut file)
            .with_context(|| format!("Failed to restore {}", relative.display()))?;
        file.sync_all()?;
    }
    Ok(())
}

/// Moves the live files into `<dir>/old/<n>`, then the staged ones into
/// place, journaling each phase first. `placed` counts the targets placed.
/// Returns the moved-aside files.
fn swap(
    dir: &Path,
    journal_path: &Path,
    journal: &mut Journal,
    exclusions: &[Vec<glob::Pattern>],
    placed: &mut usize,
) -> Result<Vec<(usize, PathBuf)>> {
    let mut aside = Vec::new();
    for (n, root) in journal.roots.iter().enumerate() {
        aside.extend(
            entries(root, &exclusions[n])?
                .into_iter()
                .map(|path| (n, path)),
        );
    }
    write_journal(journal_path, journal)?;
    for (n, relative) in &aside {
        let live = journal.roots[*n].join(relative);
        let old = dir.join("old").join(n.to_string()).join(relative);
        move_entry(&live, &old)
            .map_err(in_use)
            .with_context(|| format!("Failed to move {} aside", live.display()))?;
    }
    journal.phase = Phase::Swapping;
    write_journal(journal_path, journal)?;
    for root in &journal.roots {
        fs::create_dir_all(root)
            .with_context(|| format!("Failed to create tracked dir {}", root.display()))?;
    }
    for (n, relative) in &journal.targets {
        let live = journal.roots[*n].join(relative);
        if live.is_dir() {
            // An empty folder where the file goes; a full one fails the move.
            let _ = fs::remove_dir(&live);
        }
        move_entry(&dir.join("new").join(n.to_string()).join(relative), &live)
            .map_err(in_use)
            .with_context(|| format!("Failed to write {}", live.display()))?;
        *placed += 1;
    }
    journal.phase = Phase::Committed;
    write_journal(journal_path, journal)?;
    Ok(aside)
}

/// Puts the live roots back as they were before `journal`'s transaction:
/// removes the `placed` targets and moves `old/` back. Returns whether
/// everything went back.
fn roll_back(dir: &Path, journal: &Journal, placed: &[(usize, PathBuf)]) -> bool {
    set_rolling_back(true);
    let mut complete = true;
    if journal.phase != Phase::MovingAside {
        for (n, relative) in placed {
            let root = &journal.roots[*n];
            let live = root.join(relative);
            match failpoint().and_then(|()| fs::remove_file(&live)) {
                Ok(()) => prune_empty(root, &live),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(_) => complete = false,
            }
        }
    }
    for (n, root) in journal.roots.iter().enumerate() {
        let old = dir.join("old").join(n.to_string());
        let Ok(moved) = entries(&old, &[]) else {
            complete = false;
            continue;
        };
        for relative in moved {
            let (aside, live) = (old.join(&relative), root.join(&relative));
            if fs::symlink_metadata(&live).is_ok() {
                // A copy to another drive deletes the original last: one cut
                // short left the original in place, and the copy is a prefix
                // of it. Anything else at the live path, the game wrote since:
                // its file stays, and ours is kept out of the way.
                let settled = if is_prefix_of(&aside, &live) {
                    fs::remove_file(&aside).is_ok()
                } else {
                    let kept = txn_root(dir).join(KEPT).join(id(dir)).join(n.to_string());
                    move_entry(&aside, &kept.join(&relative)).is_ok()
                };
                if !settled {
                    complete = false;
                }
            } else if move_entry(&aside, &live).is_err() {
                complete = false;
            }
        }
    }
    set_rolling_back(false);
    complete
}

/// Settles the transactions left in `txn_root` by a crash, a killed process
/// or a rollback that couldn't finish: each is rolled back to the saves
/// before it and its leftovers removed. One whose rollback still can't finish
/// stays for the next call. Run it under the repository lock.
pub fn recover(txn_root: &Path) -> Result<Recovery> {
    let mut recovery = Recovery {
        rolled_back: 0,
        pending: Vec::new(),
    };
    let listing = match fs::read_dir(txn_root) {
        Ok(listing) => listing,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(recovery),
        Err(error) => return Err(error).context("Failed to read the transaction folder"),
    };
    let paths: Vec<PathBuf> = listing
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.file_name().is_none_or(|name| name != KEPT))
        .collect();
    for path in paths
        .iter()
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
    {
        let dir = path.with_extension("");
        let Ok(journal) = store::read_json::<Journal>(path) else {
            continue;
        };
        match journal.phase {
            Phase::Committed => {}
            Phase::MovingAside | Phase::Swapping => {
                if !roll_back(&dir, &journal, &journal.targets) {
                    recovery.pending.extend(journal.roots);
                    continue;
                }
                recovery.rolled_back += 1;
            }
        }
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_file(path);
    }
    // Folders without a journal were only being staged; stray temp files are
    // from journal writes.
    for path in &paths {
        if !path.with_extension("json").exists() {
            let _ = if path.is_dir() {
                fs::remove_dir_all(path)
            } else {
                fs::remove_file(path)
            };
        }
    }
    Ok(recovery)
}

fn txn_root(dir: &Path) -> &Path {
    dir.parent()
        .expect("a transaction folder is in the transaction root")
}

fn id(dir: &Path) -> &std::ffi::OsStr {
    dir.file_name().expect("a transaction folder has a name")
}

/// The files and links below `root`, outside `exclusions`, as paths below it.
fn entries(root: &Path, exclusions: &[glob::Pattern]) -> Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    if root.exists() {
        collect(root, Path::new(""), exclusions, &mut found)?;
    }
    Ok(found)
}

fn collect(
    dir: &Path,
    relative: &Path,
    exclusions: &[glob::Pattern],
    found: &mut Vec<PathBuf>,
) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("Failed to read {}", dir.display()))? {
        let entry = entry?;
        let entry_relative = relative.join(entry.file_name());
        if crate::exclusions::is_excluded(exclusions, &entry_relative) {
            continue;
        }
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .with_context(|| format!("Failed to inspect {}", path.display()))?;
        if metadata.is_dir() && !crate::fs_utils::is_link_or_reparse(&metadata) {
            collect(&path, &entry_relative, exclusions, found)?;
        } else {
            found.push(entry_relative);
        }
    }
    Ok(())
}

/// Moves the file (or link) `from` to `to`, which must not exist: a rename,
/// or across drives a copy, fsync and size check before the original is
/// deleted. A failed rename or delete is how a file the game holds shows:
/// nothing here opens a game's file in a way that could block the game.
fn move_entry(from: &Path, to: &Path) -> io::Result<()> {
    failpoint()?;
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    if !force_copy() {
        match store::rename_no_replace(from, to) {
            Err(error) if error.kind() == io::ErrorKind::CrossesDevices => {}
            result => return result,
        }
    }
    if crate::fs_utils::is_link_or_reparse(&fs::symlink_metadata(from)?) {
        return Err(io::Error::other(format!(
            "{} is a link, which can't be moved to another drive",
            from.display()
        )));
    }
    let moved = copy_then_delete(from, to);
    if moved.is_err() {
        let _ = fs::remove_file(to);
    }
    moved
}

/// `File::open` shares reading, writing and deleting with the game, unlike
/// the copy `std::fs::copy` makes on Windows.
fn copy_then_delete(from: &Path, to: &Path) -> io::Result<()> {
    let mut source = File::open(from)?;
    let mut target = File::create_new(to)?;
    let copied = io::copy(&mut source, &mut target)?;
    target.sync_all()?;
    if copied != source.metadata()?.len() {
        return Err(io::Error::other(format!(
            "{} changed while it was copied",
            from.display()
        )));
    }
    drop(source);
    failpoint()?;
    fs::remove_file(from)
}

/// Whether `part`'s bytes start `whole`'s, so removing `part` loses nothing.
fn is_prefix_of(part: &Path, whole: &Path) -> bool {
    let (Ok(part), Ok(whole)) = (fs::read(part), fs::read(whole)) else {
        return false;
    };
    whole.starts_with(&part)
}

fn write_journal(path: &Path, journal: &Journal) -> Result<()> {
    failpoint()?;
    store::write_json(path, journal)
}

/// Tags a sharing or lock violation (the game holds the file) as `SavesInUse`.
fn in_use(error: io::Error) -> anyhow::Error {
    let held = cfg!(windows) && matches!(error.raw_os_error(), Some(32 | 33));
    let error = anyhow::Error::from(error);
    if held {
        error.context(ErrorKind::SavesInUse)
    } else {
        error
    }
}

/// Removes the empty folders between `path` and `root`.
fn prune_empty(root: &Path, path: &Path) {
    for dir in path.ancestors().skip(1).take_while(|dir| *dir != root) {
        if fs::remove_dir(dir).is_err() {
            break;
        }
    }
}

#[cfg(test)]
#[derive(Default)]
struct Hooks {
    /// The forward operation (counted from 0) to fail.
    fail_at: Option<usize>,
    ops: usize,
    /// Treat the transaction folder as another drive.
    force_copy: bool,
    fail_rollback: bool,
    /// Stop at the failure as if the process died there.
    crash: bool,
    rolling_back: bool,
}

#[cfg(test)]
thread_local! {
    static HOOKS: std::cell::RefCell<Hooks> = std::cell::RefCell::default();
}

/// Fails where a test asked to, before a filesystem operation.
fn failpoint() -> io::Result<()> {
    #[cfg(test)]
    HOOKS.with_borrow_mut(|hooks| {
        let fail = if hooks.rolling_back {
            hooks.fail_rollback
        } else {
            hooks.ops += 1;
            hooks.fail_at == Some(hooks.ops - 1)
        };
        if fail {
            Err(io::Error::other("injected failure"))
        } else {
            Ok(())
        }
    })?;
    Ok(())
}

fn force_copy() -> bool {
    #[cfg(test)]
    {
        HOOKS.with_borrow(|hooks| hooks.force_copy)
    }
    #[cfg(not(test))]
    {
        false
    }
}

fn crashed() -> bool {
    #[cfg(test)]
    {
        HOOKS.with_borrow(|hooks| hooks.crash)
    }
    #[cfg(not(test))]
    {
        false
    }
}

fn set_rolling_back(on: bool) {
    #[cfg(test)]
    HOOKS.with_borrow_mut(|hooks| hooks.rolling_back = on);
    #[cfg(not(test))]
    let _ = on;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunks::CaptureRoot;
    use tempfile::TempDir;

    /// Two live roots, one with an excluded file and a nested folder, and a
    /// target version that adds, changes and drops files in both.
    struct Fixture {
        _temp: TempDir,
        store: Store,
        roots: [PathBuf; 2],
        txn: PathBuf,
        target: Vec<(PathBuf, PathBuf, FileEntry)>,
    }

    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let store = Store::open(&temp.path().join("repo")).unwrap();
            let roots = [temp.path().join("a"), temp.path().join("b")];
            write(&roots[0], "save1.dat", "old 1");
            write(&roots[0], "nested/save2.dat", "old 2");
            write(&roots[0], "nested/keep.cfg", "excluded");
            write(&roots[0], "stray/gone.dat", "only before");
            write(&roots[1], "x.dat", "old x");
            let source = temp.path().join("source");
            let mut target = Vec::new();
            for (n, files) in [
                &[
                    ("save1.dat", "new 1"),
                    ("nested/save2.dat", "new 2"),
                    ("fresh/save3.dat", "new 3"),
                ][..],
                &[("y.dat", "new y")][..],
            ]
            .into_iter()
            .enumerate()
            {
                let dir = source.join(n.to_string());
                for (path, contents) in files {
                    write(&dir, path, contents);
                }
                let captured = chunks::capture(
                    &store,
                    &[CaptureRoot {
                        uuid: Uuid::from_u128(n as u128),
                        path: &dir,
                        exclusions: &[],
                    }],
                    None,
                )
                .unwrap();
                target.extend(
                    captured
                        .into_iter()
                        .map(|entry| (roots[n].clone(), PathBuf::from(&entry.path), entry)),
                );
            }
            Self {
                txn: temp.path().join("txn"),
                _temp: temp,
                store,
                roots,
                target,
            }
        }

        fn prepared(&self) -> PreparedRestore {
            PreparedRestore {
                roots: BTreeMap::from([
                    (
                        self.roots[0].clone(),
                        crate::exclusions::parse(r#"["*.cfg"]"#),
                    ),
                    (self.roots[1].clone(), Vec::new()),
                ]),
                files: self.target.clone(),
            }
        }

        /// Every file in both roots, by root and `/`-separated path.
        fn live(&self) -> BTreeMap<String, String> {
            let mut files = BTreeMap::new();
            for (n, root) in self.roots.iter().enumerate() {
                for path in entries(root, &[]).unwrap() {
                    let key = format!("{n}/{}", path.to_string_lossy().replace('\\', "/"));
                    files.insert(key, fs::read_to_string(root.join(path)).unwrap());
                }
            }
            files
        }

        fn expected(&self) -> BTreeMap<String, String> {
            [
                ("0/save1.dat", "new 1"),
                ("0/nested/save2.dat", "new 2"),
                ("0/nested/keep.cfg", "excluded"),
                ("0/fresh/save3.dat", "new 3"),
                ("1/y.dat", "new y"),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
        }

        fn txn_is_empty(&self) -> bool {
            fs::read_dir(&self.txn).map_or(true, |mut listing| listing.next().is_none())
        }
    }

    fn write(root: &Path, path: &str, contents: &str) {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn with_hooks<T>(hooks: Hooks, run: impl FnOnce() -> T) -> (T, usize) {
        HOOKS.set(hooks);
        let result = run();
        let ops = HOOKS.with_borrow(|hooks| hooks.ops);
        HOOKS.set(Hooks::default());
        (result, ops)
    }

    #[test]
    fn a_shared_folder_is_never_a_restore_root() {
        let home = dirs::home_dir().unwrap();
        let dir = TrackedDir {
            id: 1,
            uuid: Uuid::nil().to_string(),
            game_id: 1,
            folder_slug: "home".into(),
            path: home.to_string_lossy().into_owned(),
            label: None,
            exclusions: "[]".into(),
            retired_at: None,
        };
        let error = clear_only(&[dir]).err().expect("refused").to_string();
        assert!(error.contains("Unsafe tracked dir"), "{error}");
    }

    #[test]
    fn a_restore_replaces_everything_but_exclusions_and_leaves_no_trace() {
        let fx = Fixture::new();
        apply(&fx.store, fx.prepared(), &fx.txn).unwrap();
        assert_eq!(fx.live(), fx.expected());
        assert!(!fx.roots[0].join("stray").exists(), "emptied folders go");
        assert!(fx.txn_is_empty());
    }

    /// Fails each filesystem operation in turn: every failure leaves the
    /// live folders exactly as they were.
    fn failure_sweep(force_copy: bool) {
        for fail_at in 0.. {
            let fx = Fixture::new();
            let before = fx.live();
            let (result, ops) = with_hooks(
                Hooks {
                    fail_at: Some(fail_at),
                    force_copy,
                    ..Hooks::default()
                },
                || apply(&fx.store, fx.prepared(), &fx.txn),
            );
            assert!(fx.txn_is_empty(), "leftovers after failing op {fail_at}");
            if ops <= fail_at {
                result.unwrap();
                assert_eq!(fx.live(), fx.expected());
                assert!(fail_at > 10, "the sweep covered too few operations");
                return;
            }
            assert!(result.is_err());
            assert_eq!(fx.live(), before, "failing op {fail_at} changed the saves");
        }
    }

    #[test]
    fn any_failure_with_renames_puts_everything_back() {
        failure_sweep(false);
    }

    #[test]
    fn any_failure_with_copies_puts_everything_back() {
        failure_sweep(true);
    }

    /// Stops at each operation as if the process died, then recovers as the
    /// next open does: the saves are back as they were.
    fn crash_sweep(force_copy: bool) {
        for fail_at in 0.. {
            let fx = Fixture::new();
            let before = fx.live();
            let (result, ops) = with_hooks(
                Hooks {
                    fail_at: Some(fail_at),
                    force_copy,
                    crash: true,
                    ..Hooks::default()
                },
                || apply(&fx.store, fx.prepared(), &fx.txn),
            );
            if ops <= fail_at {
                result.unwrap();
                return;
            }
            recover(&fx.txn).unwrap();
            assert!(fx.txn_is_empty(), "leftovers after a crash at op {fail_at}");
            assert_eq!(
                fx.live(),
                before,
                "a crash at op {fail_at} changed the saves"
            );
        }
    }

    #[test]
    fn a_crash_with_renames_is_rolled_back_on_the_next_open() {
        crash_sweep(false);
    }

    #[test]
    fn a_crash_with_copies_is_rolled_back_on_the_next_open() {
        crash_sweep(true);
    }

    #[test]
    fn a_copy_cut_short_keeps_the_original() {
        let fx = Fixture::new();
        let before = fx.live();
        let dir = fx.txn.join("cut");
        write(&dir.join("old").join("0"), "save1.dat", "old");
        let journal = Journal {
            phase: Phase::MovingAside,
            roots: fx.roots.to_vec(),
            targets: Vec::new(),
        };
        store::write_json(&dir.with_extension("json"), &journal).unwrap();

        assert_eq!(recover(&fx.txn).unwrap().rolled_back, 1);
        assert_eq!(fx.live(), before);
        assert!(fx.txn_is_empty());
    }

    #[test]
    fn a_file_written_where_one_was_moved_aside_keeps_both() {
        let fx = Fixture::new();
        let before = fx.live();
        let dir = fx.txn.join("raced");
        let aside = dir.join("old").join("0");
        write(&aside, "save1.dat", "moved aside");
        let journal = Journal {
            phase: Phase::MovingAside,
            roots: fx.roots.to_vec(),
            targets: Vec::new(),
        };
        store::write_json(&dir.with_extension("json"), &journal).unwrap();

        let recovery = recover(&fx.txn).unwrap();
        assert_eq!((recovery.rolled_back, recovery.pending.len()), (1, 0));
        assert_eq!(fx.live(), before, "the game's file stays");
        let kept = fx.txn.join(KEPT).join("raced").join("0").join("save1.dat");
        assert_eq!(fs::read_to_string(kept).unwrap(), "moved aside");
        assert!(!dir.exists() && !dir.with_extension("json").exists());
    }

    #[test]
    fn a_committed_transaction_only_leaves_cleanup() {
        let fx = Fixture::new();
        let before = fx.live();
        let dir = fx.txn.join("done");
        write(&dir.join("old").join("0"), "previous.dat", "aside");
        let journal = Journal {
            phase: Phase::Committed,
            roots: fx.roots.to_vec(),
            targets: vec![(0, PathBuf::from("save1.dat"))],
        };
        store::write_json(&dir.with_extension("json"), &journal).unwrap();

        assert_eq!(recover(&fx.txn).unwrap().rolled_back, 0);
        assert_eq!(fx.live(), before);
        assert!(fx.txn_is_empty());
    }

    #[test]
    fn a_rollback_that_fails_is_finished_later() {
        // Fail while placing the last target, after everything was moved aside.
        let (_, ops) = with_hooks(Hooks::default(), || {
            let fx = Fixture::new();
            apply(&fx.store, fx.prepared(), &fx.txn).unwrap();
        });
        let fx = Fixture::new();
        let (result, _) = with_hooks(
            Hooks {
                fail_at: Some(ops - 2),
                fail_rollback: true,
                ..Hooks::default()
            },
            || apply(&fx.store, fx.prepared(), &fx.txn),
        );
        let error = result.unwrap_err();
        assert_eq!(
            crate::error::error_kind(&error),
            Some(ErrorKind::RestoreIncomplete)
        );
        let journals: Vec<_> = fs::read_dir(&fx.txn)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect();
        assert_eq!(journals.len(), 1);
        let old = journals[0].with_extension("").join("old").join("0");
        assert_eq!(fs::read_to_string(old.join("save1.dat")).unwrap(), "old 1");

        let before = with_hooks(
            Hooks {
                fail_rollback: true,
                ..Hooks::default()
            },
            || recover(&fx.txn).unwrap(),
        )
        .0;
        assert_eq!(before.pending.len(), 2, "still held: kept for the next try");
        assert!(old.join("save1.dat").exists());

        // Once the files are free, the next try puts everything back.
        let recovery = recover(&fx.txn).unwrap();
        assert_eq!((recovery.rolled_back, recovery.pending.len()), (1, 0));
        assert_eq!(fx.live(), Fixture::new().live());
        assert!(fx.txn_is_empty());
    }
}
