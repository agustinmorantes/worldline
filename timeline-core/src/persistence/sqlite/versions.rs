use super::SqliteVersionRepository;
use crate::models::{FullVersion, Game, Slot, Version, VersionForm};
use crate::persistence::VersionRepository;
use anyhow::{Context, Result};
use diesel::prelude::*;
use uuid::Uuid;

impl VersionRepository for SqliteVersionRepository {
    fn create_version(&self, game_id_param: i32) -> Result<FullVersion> {
        use crate::schema::games;
        use crate::schema::slots;
        use crate::schema::versions;

        let conn = &mut self.pool.get().context("Error getting db connection")?;

        let game_model: Game = games::dsl::games
            .filter(games::dsl::id.eq(game_id_param))
            .select(Game::as_select())
            .first(conn)
            .context("Error fetching game for version creation")?;

        let active_slot_id = game_model
            .active_slot_id
            .context("Game is missing an active slot")?;
        let slot_model: Slot = slots::dsl::slots
            .filter(slots::dsl::id.eq(active_slot_id))
            .select(Slot::as_select())
            .first(conn)
            .context("Error fetching active slot for version creation")?;

        // Use MAX(version_number) + 1 so version numbers never collide after branching
        let new_version_number: i32 = {
            use diesel::dsl::max;
            versions::dsl::versions
                .filter(versions::dsl::slot_id.eq(active_slot_id))
                .select(max(versions::dsl::version_number))
                .first::<Option<i32>>(conn)
                .context("Error fetching max version number")?
                .unwrap_or(0)
                + 1
        };

        let new_version = VersionForm {
            id: None,
            uuid: Some(Uuid::new_v4().to_string()),
            folder_slug: None,
            slot_id: active_slot_id,
            version_number: new_version_number,
            tag: None,
            created_at: None,
            parent_id: slot_model.active_version_id,
        };
        let version_uuid = new_version.uuid.clone().expect("version uuid should exist");
        let version_uuid = Uuid::parse_str(&version_uuid).expect("version uuid should parse");
        let new_version = VersionForm {
            folder_slug: Some(crate::repository::build_folder_slug(
                crate::repository::RepositoryEntityKind::Version,
                &format!("v{:03}", new_version_number),
                version_uuid,
            )),
            ..new_version
        };

        // Insert the new version
        let version: Version = diesel::insert_into(versions::dsl::versions)
            .values(&new_version)
            .returning(Version::as_returning())
            .get_result(conn)
            .context("Error inserting new version")?;

        let updated_slot: Slot =
            diesel::update(slots::dsl::slots.filter(slots::dsl::id.eq(active_slot_id)))
                .set(slots::dsl::active_version_id.eq(version.id))
                .returning(Slot::as_returning())
                .get_result(conn)
                .context("Error updating slot active version")?;

        let game: Game = games::dsl::games
            .filter(games::dsl::id.eq(updated_slot.game_id))
            .select(Game::as_select())
            .first(conn)
            .context("Error loading game for version")?;

        let full_version = FullVersion {
            version,
            game,
            slot: updated_slot,
        };

        if let Some(repository_root) = &self.repository_root {
            crate::repository::sync_version_metadata(repository_root, &full_version)
                .context("Error writing version metadata")?;
            let full_game = load_full_game(conn, game_id_param)
                .context("Error loading full game for metadata sync")?;
            crate::repository::sync_game_metadata(repository_root, &full_game)
                .context("Error writing game metadata")?;
        }

        Ok(full_version)
    }

    fn set_active_version(&self, version_id: i32) -> Result<FullVersion> {
        use crate::schema::games;
        use crate::schema::slots;
        use crate::schema::versions;

        let conn = &mut self.pool.get().context("Error getting db connection")?;

        let version: Version = versions::dsl::versions
            .filter(versions::dsl::id.eq(version_id))
            .select(Version::as_select())
            .first(conn)
            .context("Error loading version to activate")?;

        let slot: Slot =
            diesel::update(slots::dsl::slots.filter(slots::dsl::id.eq(version.slot_id)))
                .set(slots::dsl::active_version_id.eq(version.id))
                .returning(Slot::as_returning())
                .get_result(conn)
                .context("Error updating slot active version")?;

        let game: Game = diesel::update(games::dsl::games.filter(games::dsl::id.eq(slot.game_id)))
            .set(games::dsl::active_slot_id.eq(slot.id))
            .returning(Game::as_returning())
            .get_result(conn)
            .context("Error updating game active version")?;

        let full_version = FullVersion {
            version,
            game,
            slot,
        };

        if let Some(repository_root) = &self.repository_root {
            crate::repository::sync_version_metadata(repository_root, &full_version)
                .context("Error writing version metadata")?;
            let full_game = load_full_game(conn, full_version.game.id)
                .context("Error loading full game for metadata sync")?;
            crate::repository::sync_game_metadata(repository_root, &full_game)
                .context("Error writing game metadata")?;
        }

        Ok(full_version)
    }

    fn tag_version(&self, target_id: i32, tag_value: String) -> Result<Version> {
        use crate::schema::versions::dsl as versions_dsl;

        let conn = &mut self.pool.get().context("Error getting db connection")?;

        let updated_version: Version =
            diesel::update(versions_dsl::versions.filter(versions_dsl::id.eq(target_id)))
                .set(versions_dsl::tag.eq(tag_value))
                .returning(Version::as_returning())
                .get_result(conn)
                .context("Error tagging version")?;

        Ok(updated_version)
    }

    fn get_version(&self, version_id: i32) -> Result<FullVersion> {
        use crate::schema::games::dsl as games_dsl;
        use crate::schema::slots::dsl as slots_dsl;
        use crate::schema::versions::dsl as versions_dsl;

        let conn = &mut self.pool.get().context("Error getting db connection")?;

        let version: Version = versions_dsl::versions
            .filter(versions_dsl::id.eq(version_id))
            .select(Version::as_select())
            .first(conn)
            .context("Error fetching version")?;

        let slot: Slot = slots_dsl::slots
            .filter(slots_dsl::id.eq(version.slot_id))
            .select(Slot::as_select())
            .first(conn)
            .context("Error fetching slot for version")?;

        let game: Game = games_dsl::games
            .filter(games_dsl::id.eq(slot.game_id))
            .select(Game::as_select())
            .first(conn)
            .context("Error fetching game for version")?;

        Ok(FullVersion {
            version,
            game,
            slot,
        })
    }
}

fn load_full_game(conn: &mut diesel::SqliteConnection, id: i32) -> Result<crate::models::FullGame> {
    use crate::schema::games;
    use crate::schema::versions;

    let game: Game = games::dsl::games
        .filter(games::dsl::id.eq(id))
        .select(Game::as_select())
        .first(conn)
        .context("Error loading game")?;

    let dirs: Vec<crate::models::TrackedDir> = crate::models::TrackedDir::belonging_to(&game)
        .select(crate::models::TrackedDir::as_select())
        .load(conn)?;

    let slots_for_game: Vec<Slot> = Slot::belonging_to(&game)
        .select(Slot::as_select())
        .order_by(crate::schema::slots::dsl::created_at.asc())
        .load(conn)?;

    let loaded_versions: Vec<Version> = Version::belonging_to(&slots_for_game)
        .select(Version::as_select())
        .order_by(versions::dsl::created_at.desc())
        .load(conn)?;

    let active_slot = slots_for_game
        .iter()
        .find(|slot| Some(slot.id) == game.active_slot_id)
        .cloned();
    let active_slot_versions = if let Some(slot) = active_slot.as_ref() {
        loaded_versions
            .iter()
            .filter(|version| version.slot_id == slot.id)
            .cloned()
            .collect()
    } else {
        Vec::new()
    };

    Ok(crate::models::FullGame {
        last_version: if let Some(slot) = active_slot.as_ref() {
            active_slot_versions
                .iter()
                .find(|v| Some(v.id) == slot.active_version_id)
                .cloned()
        } else {
            None
        },
        tracked_dirs: dirs,
        slots: slots_for_game,
        active_slot,
        game,
        versions: active_slot_versions,
    })
}
