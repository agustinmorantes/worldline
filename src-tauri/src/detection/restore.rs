use std::path::{Path, PathBuf};
use crate::DbPool;
use crate::detection::{Command, CommandSender};
use anyhow::{Context, Result};
use fs_extra::dir::CopyOptions;
use crate::persistence::games::{get_game, get_games};
use crate::persistence::versions::{create_version, get_version};

pub fn restore_version(version_id: i32, db: DbPool, cmd_tx: CommandSender, working_base: &Path) -> Result<()> {
    cmd_tx.send(Command::Pause)
        .context("Failed to send pause command")?;

    let version = get_version(version_id, db.clone()).context("Failed to get version")?;
    let game = get_game(version.game.id, db.clone()).context("Failed to get game")?;
    let dirs = game.tracked_dirs;

    create_version(game.game.id, db.clone(), cmd_tx.clone())
        .context("Failed to create new version for restoration")?;

    cmd_tx.send(Command::Pause)
        .context("Failed to send pause command")?;

    let paths: Vec<_> = dirs.iter()
        .map(|dir| &dir.path)
        .collect();

    fs_extra::remove_items(&paths).ok();

    for dir in &dirs {
        let from = working_base
                .join(game.game.id.to_string())
                .join(version.version.version_number.to_string())
                .join(dir.id.to_string());

        let to = PathBuf::from(&dir.path);

        let mut options = CopyOptions::new();
        options.copy_inside = true;
        options.overwrite = true;
        fs_extra::dir::copy(from, to, &options)
            .context("Failed to copy directory")?;
    }


    cmd_tx.send(Command::ReregisterGames(
        get_games(db.clone())?
    ))
        .context("Failed to send pause command")?;

    cmd_tx.send(Command::Resume)
        .context("Failed to send resume command")?;

    println!("Restored version {} for game {}", version_id, game.game.id);

    Ok(())
}
