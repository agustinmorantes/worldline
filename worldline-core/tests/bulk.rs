//! Acting on several games at once: delete everywhere, recover (the batch
//! undo) and remove from this PC.

mod common;

use common::Device;
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;
use worldline_core::error::ErrorKind;
use worldline_core::models::{FullGame, GameOutcome, GameResult};
use worldline_core::store::Store;

fn uuid_of(game: &FullGame) -> Uuid {
    Uuid::parse_str(&game.game.uuid).unwrap()
}

/// Three games, each tracking its own folder holding `save.dat`.
fn three_games(device: &Device) -> (Vec<FullGame>, Vec<PathBuf>) {
    let mut games = Vec::new();
    let mut dirs = Vec::new();
    for title in ["Alpha", "Beta", "Gamma"] {
        let dir = device.saves.path().join(title);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("save.dat"), format!("{title} 1")).unwrap();
        games.push(device.add_game_at(title, &dir));
        dirs.push(dir);
    }
    (games, dirs)
}

fn outcomes(results: &[GameResult]) -> Vec<(Option<i32>, GameOutcome)> {
    results
        .iter()
        .map(|result| (result.game_id, result.outcome.clone()))
        .collect()
}

fn titles(device: &Device) -> Vec<String> {
    let mut titles: Vec<_> = device
        .core
        .get_games()
        .unwrap()
        .into_iter()
        .map(|game| game.game.title)
        .collect();
    titles.sort();
    titles
}

/// Makes a game's metadata unreadable, so acting on it fails; returns what
/// puts it back.
fn break_game(device: &Device, game: &FullGame) -> impl FnOnce() {
    let path = device
        .core
        .repository_root()
        .join("games")
        .join(&game.game.folder_slug)
        .join("metadata.json");
    let original = fs::read(&path).unwrap();
    fs::write(&path, "not json").unwrap();
    move || fs::write(path, original).unwrap()
}

#[test]
fn deleting_several_games_reports_each_once_and_keeps_every_one_recoverable() {
    let device = Device::new();
    let (games, dirs) = three_games(&device);
    let [alpha, beta, gamma] = [&games[0], &games[1], &games[2]];
    fs::write(dirs[0].join("save.dat"), "Alpha 2").unwrap();
    let alpha_v2 = device.checkpoint(alpha.game.id);
    // Progress made since the last checkpoint stays on disk, unrecorded.
    fs::write(dirs[1].join("save.dat"), "Beta unrecorded").unwrap();

    let results = device
        .core
        .delete_games(&[alpha.game.id, beta.game.id, alpha.game.id, 9999])
        .unwrap();
    assert_eq!(
        outcomes(&results),
        vec![
            (Some(alpha.game.id), GameOutcome::Done),
            (Some(beta.game.id), GameOutcome::Done),
            (Some(9999), GameOutcome::NotFound),
        ]
    );
    assert_eq!(results[0].title, "Alpha");
    assert_eq!(results[0].uuid, Some(uuid_of(alpha)));
    assert_eq!(titles(&device), vec!["Gamma"]);
    let deleted: Vec<_> = device
        .core
        .recently_deleted()
        .unwrap()
        .into_iter()
        .map(|item| item.uuid)
        .collect();
    assert_eq!(deleted.len(), 2);
    assert!(deleted.contains(&uuid_of(alpha)) && deleted.contains(&uuid_of(beta)));
    // Deleting never writes live saves.
    assert_eq!(
        fs::read_to_string(dirs[0].join("save.dat")).unwrap(),
        "Alpha 2"
    );
    assert_eq!(
        fs::read_to_string(dirs[1].join("save.dat")).unwrap(),
        "Beta unrecorded"
    );

    // The batch undo brings both back where this device was, again without writing live saves.
    let results = device
        .core
        .recover_games(&[uuid_of(alpha), uuid_of(beta), uuid_of(alpha)])
        .unwrap();
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| r.outcome == GameOutcome::Done));
    assert_eq!(titles(&device), vec!["Alpha", "Beta", "Gamma"]);
    let alpha_back = device.game_by_title("Alpha").unwrap();
    assert_eq!(results[0].game_id, Some(alpha_back.game.id));
    assert_eq!(
        device.active_version(alpha_back.game.id).uuid,
        alpha_v2.uuid,
        "each game resumes from the pointers saved before its own deletion"
    );
    assert_eq!(
        fs::read_to_string(dirs[1].join("save.dat")).unwrap(),
        "Beta unrecorded"
    );
    assert!(device.core.recently_deleted().unwrap().is_empty());
    assert!(device.game_by_title("Gamma").unwrap().game.id == gamma.game.id);
}

#[test]
fn a_game_that_fails_to_delete_stays_and_the_others_still_go() {
    let device = Device::new();
    let (games, _) = three_games(&device);
    let restore = break_game(&device, &games[1]);

    let results = device
        .core
        .delete_games(&[games[0].game.id, games[1].game.id, games[2].game.id])
        .unwrap();
    assert_eq!(results[0].outcome, GameOutcome::Done);
    assert!(
        matches!(&results[1].outcome, GameOutcome::Failed { failure } if failure.message.contains("missing")),
        "{:?}",
        results[1]
    );
    assert_eq!(results[2].outcome, GameOutcome::Done);
    assert_eq!(titles(&device), vec!["Beta"]);

    // Retrying only the failure deletes it; the others aren't in the library any more.
    restore();
    let retry = device
        .core
        .delete_games(&[games[0].game.id, games[1].game.id])
        .unwrap();
    assert_eq!(
        outcomes(&retry),
        vec![
            (Some(games[0].game.id), GameOutcome::NotFound),
            (Some(games[1].game.id), GameOutcome::Done),
        ]
    );
    assert!(titles(&device).is_empty());
    assert_eq!(device.core.recently_deleted().unwrap().len(), 3);
}

#[test]
fn recovering_a_batch_reports_failures_truthfully_and_a_retry_redoes_only_them() {
    let device = Device::new();
    let (games, dirs) = three_games(&device);
    let ids: Vec<i32> = games.iter().map(|game| game.game.id).collect();
    device.core.delete_games(&ids[..2]).unwrap();
    // Another game now tracks Alpha's folder, so Alpha can't come back yet.
    let squatter = device.add_game_at("Squatter", &dirs[0]);

    let uuids = [uuid_of(&games[0]), uuid_of(&games[1]), Uuid::new_v4()];
    let results = device.core.recover_games(&uuids).unwrap();
    assert!(
        matches!(&results[0].outcome, GameOutcome::Failed { failure } if failure.kind == Some(ErrorKind::FolderTaken)),
        "{:?}",
        results[0]
    );
    assert_eq!(results[0].game_id, None);
    assert_eq!(results[0].title, "Alpha");
    assert_eq!(results[1].outcome, GameOutcome::Done);
    assert_eq!(results[2].outcome, GameOutcome::NotFound);
    assert_eq!(titles(&device), vec!["Beta", "Gamma", "Squatter"]);
    let store = Store::open_existing(device.core.repository_root()).unwrap();
    let beta_recovered = store.find_game(uuids[1]).unwrap().unwrap().deletion;

    // Retrying the whole batch recovers Alpha and leaves Beta as it was.
    device.core.delete_games(&[squatter.game.id]).unwrap();
    let retry = device.core.recover_games(&uuids[..2]).unwrap();
    assert!(
        retry.iter().all(|r| r.outcome == GameOutcome::Done),
        "{retry:?}"
    );
    assert_eq!(titles(&device), vec!["Alpha", "Beta", "Gamma"]);
    assert_eq!(
        store.find_game(uuids[1]).unwrap().unwrap().deletion,
        beta_recovered,
        "an already recovered game is not written again"
    );
}

#[test]
fn removing_several_games_from_this_pc_skips_those_not_in_the_cloud() {
    let remote = tempfile::tempdir().unwrap();
    let device = Device::synced(remote.path());
    let (games, dirs) = three_games(&device);
    device.core.push_sync().unwrap();
    let late = tempfile::tempdir().unwrap();
    fs::write(late.path().join("save.dat"), "late").unwrap();
    let local_only = device.add_game_at("Local only", late.path());
    // Uncaptured progress is recorded before its game goes.
    fs::write(dirs[0].join("save.dat"), "Alpha 2").unwrap();

    let results = device
        .core
        .untrack_games(&[
            games[0].game.id,
            local_only.game.id,
            games[1].game.id,
            games[0].game.id,
            9999,
        ])
        .unwrap();
    assert_eq!(
        outcomes(&results),
        vec![
            (Some(games[0].game.id), GameOutcome::Done),
            (Some(local_only.game.id), GameOutcome::NotInCloud),
            (Some(games[1].game.id), GameOutcome::Done),
            (Some(9999), GameOutcome::NotFound),
        ]
    );
    assert_eq!(results[1].title, "Local only");
    assert_eq!(titles(&device), vec!["Gamma", "Local only"]);
    let on_remote = Store::open_existing(remote.path())
        .unwrap()
        .game(uuid_of(&games[0]))
        .unwrap()
        .unwrap();
    assert_eq!(
        on_remote.versions().count(),
        2,
        "Alpha's progress was published"
    );
    assert!(
        device.core.recently_deleted().unwrap().is_empty(),
        "removing from this PC is not a deletion"
    );
    assert_eq!(
        fs::read_to_string(dirs[0].join("save.dat")).unwrap(),
        "Alpha 2"
    );
    let report = device.core.refresh_sync().unwrap();
    for game in &games[..2] {
        let listed = report
            .remote_games
            .iter()
            .find(|g| g.uuid == uuid_of(game))
            .unwrap();
        assert!(!listed.adopted, "back in the cloud list: {listed:?}");
    }
}

#[test]
fn without_sync_no_game_can_be_removed_from_this_pc() {
    let device = Device::new();
    let (games, _) = three_games(&device);
    let results = device
        .core
        .untrack_games(&[games[0].game.id, games[1].game.id])
        .unwrap();
    assert!(results.iter().all(|r| r.outcome == GameOutcome::NotInCloud));
    assert_eq!(titles(&device).len(), 3);
}

#[test]
fn a_game_that_fails_to_be_removed_stays_tracked_and_the_others_still_go() {
    let remote = tempfile::tempdir().unwrap();
    let device = Device::synced(remote.path());
    let (games, dirs) = three_games(&device);
    device.core.push_sync().unwrap();
    // Changed saves need a checkpoint first, which can't read the broken game.
    fs::write(dirs[1].join("save.dat"), "Beta 2").unwrap();
    let restore = break_game(&device, &games[1]);

    let results = device
        .core
        .untrack_games(&[games[0].game.id, games[1].game.id])
        .unwrap();
    assert_eq!(results[0].outcome, GameOutcome::Done);
    assert!(
        matches!(results[1].outcome, GameOutcome::Failed { .. }),
        "{:?}",
        results[1]
    );
    // Once its files read again, Beta is still tracked here; Alpha is not.
    restore();
    device.core.reindex().unwrap();
    assert_eq!(titles(&device), vec!["Beta", "Gamma"]);
}
