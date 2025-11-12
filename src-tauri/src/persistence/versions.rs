use crate::models::{FullVersion, Game, Version, VersionForm};
use crate::persistence::games::{get_game, get_games};
use crate::DbPool;
use anyhow::{Context, Result};
use diesel::prelude::*;
use crate::detection::{Command, CommandSender};

pub fn create_version(game_id: i32, db: DbPool, cmd_tx: CommandSender) -> Result<FullVersion> {
    use crate::schema::versions;
    use crate::schema::games;

    let conn = &mut db
        .get()
        .context("Error getting db connection")?;

    let game = get_game(game_id, db.clone())
        .context("Error fetching game for version creation")?;

    let new_version = VersionForm {
        id: None,
        game_id,
        version_number: game.game.active_version_id.map_or(1, |v_id| v_id),
        tag: None,
        created_at: None,
    };

    // Insert the new version
    let version: Version = diesel::insert_into(versions::dsl::versions)
        .values(&new_version)
        .returning(Version::as_returning())
        .get_result(conn)
        .context("Error inserting new version")?;

    // Update the game's active_version_id
    let game = diesel::update(games::dsl::games.filter(games::dsl::id.eq(game_id)))
        .set(games::dsl::active_version_id.eq(version.version_number + 1))
        .get_result(conn)
        .context("Error updating game active version")?;

    let all_games = get_games(db.clone())?;

    // Reregister games
    cmd_tx.send(Command::ReregisterGames(all_games))
        .context("Error sending ReregisterGames command")?;

    Ok(FullVersion {
        version,
        game,
    })
}

pub fn get_version(version_id: i32, db: DbPool) -> Result<FullVersion> {
    use crate::schema::versions::dsl as versions_dsl;
    use crate::schema::games::dsl as games_dsl;

    let conn = &mut db
        .get()
        .context("Error getting db connection")?;

    let version: Version = versions_dsl::versions
        .filter(versions_dsl::id.eq(version_id))
        .select(Version::as_select())
        .first(conn)
        .context("Error fetching version")?;

    let game: crate::models::Game = games_dsl::games
        .filter(games_dsl::id.eq(version.game_id))
        .select(Game::as_select())
        .first(conn)
        .context("Error fetching game for version")?;

    Ok(FullVersion {
        version,
        game,
    })
}
