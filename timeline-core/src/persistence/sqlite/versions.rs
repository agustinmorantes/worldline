use crate::models::{FullVersion, Game, Version, VersionForm};
use anyhow::{Context, Result};
use diesel::prelude::*;
use super::SqliteVersionRepository;
use crate::persistence::VersionRepository;

impl VersionRepository for SqliteVersionRepository {
    fn create_version(&self, game_id_param: i32) -> Result<FullVersion> {
        use crate::schema::versions;
        use crate::schema::games;

        let conn = &mut self.pool
            .get()
            .context("Error getting db connection")?;

        let game_model: Game = games::dsl::games
            .filter(games::dsl::id.eq(game_id_param))
            .select(Game::as_select())
            .first(conn)
            .context("Error fetching game for version creation")?;

        // Use MAX(version_number) + 1 so version numbers never collide after branching
        let new_version_number: i32 = {
            use diesel::dsl::max;
            versions::dsl::versions
                .filter(versions::dsl::game_id.eq(game_id_param))
                .select(max(versions::dsl::version_number))
                .first::<Option<i32>>(conn)
                .context("Error fetching max version number")?
                .unwrap_or(0)
                + 1
        };

        let new_version = VersionForm {
            id: None,
            game_id: game_id_param,
            version_number: new_version_number,
            tag: None,
            created_at: None,
            parent_id: game_model.active_version_id,
        };

        // Insert the new version
        let version: Version = diesel::insert_into(versions::dsl::versions)
            .values(&new_version)
            .returning(Version::as_returning())
            .get_result(conn)
            .context("Error inserting new version")?;

        // Update the game's active_version_id to the NEW version's ID
        let updated_game: Game = diesel::update(games::dsl::games.filter(games::dsl::id.eq(game_id_param)))
            .set(games::dsl::active_version_id.eq(version.id))
            .returning(Game::as_returning())
            .get_result(conn)
            .context("Error updating game active version")?;

        Ok(FullVersion {
            version,
            game: updated_game,
        })
    }

    fn tag_version(&self, target_id: i32, tag_value: String) -> Result<Version> {
        use crate::schema::versions::dsl as versions_dsl;

        let conn = &mut self.pool
            .get()
            .context("Error getting db connection")?;

        let updated_version: Version = diesel::update(versions_dsl::versions.filter(versions_dsl::id.eq(target_id)))
            .set(versions_dsl::tag.eq(tag_value))
            .returning(Version::as_returning())
            .get_result(conn)
            .context("Error tagging version")?;

        Ok(updated_version)
    }

    fn get_version(&self, version_id: i32) -> Result<FullVersion> {
        use crate::schema::versions::dsl as versions_dsl;
        use crate::schema::games::dsl as games_dsl;
    
        let conn = &mut self.pool
            .get()
            .context("Error getting db connection")?;
    
        let version: Version = versions_dsl::versions
            .filter(versions_dsl::id.eq(version_id))
            .select(Version::as_select())
            .first(conn)
            .context("Error fetching version")?;
    
        let game: Game = games_dsl::games
            .filter(games_dsl::id.eq(version.game_id))
            .select(Game::as_select())
            .first(conn)
            .context("Error fetching game for version")?;
    
        Ok(FullVersion {
            version,
            game,
        })
    }
}
