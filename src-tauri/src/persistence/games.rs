use crate::{detection, DbPool};
use crate::models::*;
use anyhow::{Context, Result};
use diesel::prelude::*;
use crate::detection::CommandSender;

pub fn get_games(db: DbPool) -> Result<Vec<FullGame>> {
    use crate::schema::games::dsl::*;

    let conn = &mut db
        .get()
        .context("Error getting db connection")?;

    let all_games = games
        .select(Game::as_select())
        .load(conn)
        .context("Error loading games")?;

    let dirs = TrackedDir::belonging_to(&all_games)
        .select(TrackedDir::as_select())
        .load(conn)?;

    let versions = Version::belonging_to(&all_games)
        .select(Version::as_select())
        .load(conn)?;

    let res = dirs
        .grouped_by(&all_games)
        .into_iter()
        .zip(versions.grouped_by(&all_games))
        .zip(all_games)
        .map(|((dirs, versions), game)| FullGame {
            tracked_dirs: dirs,
            last_version: versions
                .iter()
                .find(|v| Some(v.version_number + 1) == game.active_version_id)
                .cloned(),
            versions,
            game,
        })
        .collect();

    Ok(res)
}

pub fn get_game(id: i32, db: DbPool) -> Result<FullGame> {
    use crate::schema::games;
    use crate::schema::versions;

    let conn = &mut db
        .get()
        .context("Error getting db connection")?;

    let game: Game = games::dsl::games
        .filter(games::dsl::id.eq(id))
        .select(Game::as_select())
        .first(conn)
        .context("Error loading game")?;

    let dirs: Vec<TrackedDir> = TrackedDir::belonging_to(&game)
        .select(TrackedDir::as_select())
        .load(conn)?;

    let versions: Vec<Version> = Version::belonging_to(&game)
        .select(Version::as_select())
        .order_by(versions::dsl::created_at.desc())
        .load(conn)?;

    Ok(FullGame {
        last_version: versions
            .iter()
            .find(|v| Some(v.version_number + 1) == game.active_version_id)
            .cloned(),
        tracked_dirs: dirs,
        game,
        versions,
    })
}
pub fn save_game(
    game: GameForm,
    dirs: Option<Vec<&str>>,
    db: DbPool,
    cmd_tx: CommandSender,
) -> Result<Game> {
    use crate::schema::games;
    use crate::schema::tracked_dirs;
    use crate::schema::tracked_dirs::dsl;

    let conn = &mut db
        .get()
        .context("Error getting db connection")?;

    let res = diesel::insert_into(games::table)
        .values(&game)
        .on_conflict(games::id)
        .do_update()
        .set(&game)
        .returning(Game::as_returning())
        .get_result(conn)
        .context("Error saving new game")?;

    if let Some(dirs) = dirs {
        diesel::delete(
            dsl::tracked_dirs.filter(
                tracked_dirs::game_id
                    .eq(res.id)
                    .and(tracked_dirs::path.ne_all(&dirs)),
            ),
        )
            .execute(conn)
            .context("Error deleting old tracked dirs")?;

        let new_dirs: Vec<TrackedDirForm> = dirs
            .iter()
            .map(|path| TrackedDirForm {
                game_id: res.id,
                path,
                id: None,
            })
            .collect();

        // Can't do bulk insert with on_conflict do nothing.
        for dir in &new_dirs {
            diesel::insert_into(tracked_dirs::table)
                .values(dir)
                .on_conflict(tracked_dirs::path)
                .do_nothing()
                .execute(conn)
                .context("Error saving tracked dirs")?;
        }
    }

    // Notify detection system of updated games & dirs
    detection::reregister_games(&cmd_tx, &db)?;

    Ok(res)
}

pub fn delete_game(id: i32, db: DbPool, cmd_tx: CommandSender) -> Result<()> {
    use crate::schema::games::dsl;

    let conn = &mut db
        .get()
        .context("Error getting db connection")?;

    diesel::delete(dsl::games.filter(dsl::id.eq(id)))
        .execute(conn)
        .context("Error deleting game")?;

    detection::reregister_games(&cmd_tx, &db)?;

    Ok(())
}
