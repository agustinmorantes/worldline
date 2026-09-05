use super::SqliteGameRepository;
use crate::models::*;
use crate::persistence::GameRepository;
use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use diesel::prelude::*;
use diesel::{AsChangeset, Insertable};
use std::path::Path;
use uuid::Uuid;

#[derive(Insertable, AsChangeset)]
#[diesel(table_name = crate::schema::games)]
struct PersistedGameForm {
    id: Option<i32>,
    uuid: Option<String>,
    title: String,
    title_normalized: Option<String>,
    folder_slug: Option<String>,
    active_slot_id: Option<i32>,
    icon_path: Option<String>,
}

impl GameRepository for SqliteGameRepository {
    fn get_games(&self) -> Result<Vec<FullGame>> {
        use crate::schema::games::dsl::*;

        let conn = &mut self.pool.get().context("Error getting db connection")?;

        let all_games = games
            .select(Game::as_select())
            .load(conn)
            .context("Error loading games")?;

        let dirs = TrackedDir::belonging_to(&all_games)
            .select(TrackedDir::as_select())
            .load(conn)?;

        let all_slots = Slot::belonging_to(&all_games)
            .select(Slot::as_select())
            .load(conn)?;

        let loaded_versions = Version::belonging_to(&all_slots)
            .select(Version::as_select())
            .load(conn)?;

        let versions_by_slot: std::collections::HashMap<i32, Vec<Version>> = all_slots
            .iter()
            .zip(loaded_versions.grouped_by(&all_slots))
            .map(|(slot, versions)| (slot.id, versions))
            .collect();

        let res = dirs
            .grouped_by(&all_games)
            .into_iter()
            .zip(all_slots.grouped_by(&all_games))
            .zip(all_games)
            .map(|((dirs, slots), game)| {
                let active_slot = slots
                    .iter()
                    .find(|slot| Some(slot.id) == game.active_slot_id)
                    .cloned();
                let active_slot_versions = active_slot
                    .as_ref()
                    .and_then(|slot| versions_by_slot.get(&slot.id))
                    .cloned()
                    .unwrap_or_default();
                let last_version = if let Some(slot) = active_slot.as_ref() {
                    active_slot_versions
                        .iter()
                        .find(|v| Some(v.id) == slot.active_version_id)
                        .cloned()
                } else {
                    None
                };

                FullGame {
                    tracked_dirs: dirs,
                    slots,
                    active_slot,
                    last_version,
                    versions: active_slot_versions,
                    game,
                }
            })
            .collect();

        Ok(res)
    }

    fn get_game(&self, id: i32) -> Result<FullGame> {
        let conn = &mut self.pool.get().context("Error getting db connection")?;
        load_full_game(conn, id)
    }

    fn get_slot(&self, id: i32) -> Result<FullSlot> {
        let conn = &mut self.pool.get().context("Error getting db connection")?;
        load_full_slot(conn, id)
    }

    fn list_slots(&self, game_id: i32) -> Result<Vec<FullSlot>> {
        let conn = &mut self.pool.get().context("Error getting db connection")?;
        load_full_slots_for_game(conn, game_id)
    }

    fn create_slot(&self, game_id: i32, name: &str) -> Result<FullSlot> {
        use crate::schema::games::dsl as games_dsl;
        use crate::schema::slots::dsl as slots_dsl;

        let created_slot = {
            let conn = &mut self.pool.get().context("Error getting db connection")?;

            let game: Game = games_dsl::games
                .filter(games_dsl::id.eq(game_id))
                .select(Game::as_select())
                .first(conn)
                .context("Error loading game for slot creation")?;

            let normalized_name = crate::repository::normalize_display_name(name);
            let duplicate_slot = slots_dsl::slots
                .filter(slots_dsl::game_id.eq(game.id))
                .filter(slots_dsl::name_normalized.eq(&normalized_name))
                .select(Slot::as_select())
                .first(conn)
                .optional()
                .context("Error checking for duplicate slot name")?;

            if duplicate_slot.is_some() {
                return Err(anyhow!(
                    "A slot named '{name}' already exists for this game"
                ));
            }

            let slot_uuid = Uuid::new_v4();
            let now = Utc::now().naive_utc();
            let slot_form = SlotForm {
                id: None,
                uuid: slot_uuid.to_string(),
                game_id: game.id,
                name: name.to_string(),
                name_normalized: normalized_name,
                folder_slug: crate::repository::build_folder_slug(
                    crate::repository::RepositoryEntityKind::Slot,
                    name,
                    slot_uuid,
                ),
                active_version_id: None,
                created_at: Some(now),
                updated_at: Some(now),
            };

            diesel::insert_into(crate::schema::slots::table)
                .values(&slot_form)
                .returning(Slot::as_returning())
                .get_result(conn)
                .context("Error creating slot")?
        };

        sync_game_metadata_if_needed(self, game_id)?;
        self.get_slot(created_slot.id)
    }

    fn set_active_slot(&self, game_id: i32, slot_id: i32) -> Result<Slot> {
        use crate::schema::games::dsl as games_dsl;
        use crate::schema::slots::dsl as slots_dsl;

        let slot = {
            let conn = &mut self.pool.get().context("Error getting db connection")?;

            let slot: Slot = slots_dsl::slots
                .filter(slots_dsl::id.eq(slot_id))
                .filter(slots_dsl::game_id.eq(game_id))
                .select(Slot::as_select())
                .first(conn)
                .context("Error loading slot to activate")?;

            diesel::update(games_dsl::games.filter(games_dsl::id.eq(game_id)))
                .set(games_dsl::active_slot_id.eq(slot.id))
                .execute(conn)
                .context("Error updating active slot")?;

            slot
        };

        sync_game_metadata_if_needed(self, game_id)?;
        Ok(slot)
    }

    fn save_game(&self, form: GameForm, dirs: Option<Vec<&str>>) -> Result<Game> {
        use crate::schema::games;
        use crate::schema::tracked_dirs;
        use crate::schema::tracked_dirs::dsl;

        let res = {
            let conn = &mut self.pool.get().context("Error getting db connection")?;
            let enriched_form = enrich_game_form(conn, form)?;

            let mut res = diesel::insert_into(games::table)
                .values(&enriched_form)
                .on_conflict(games::id)
                .do_update()
                .set(&enriched_form)
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
                    .map(|path| {
                        let uuid = Uuid::new_v4();
                        TrackedDirForm {
                            uuid: Some(uuid.to_string()),
                            folder_slug: Some(crate::repository::build_folder_slug(
                                crate::repository::RepositoryEntityKind::TrackedDir,
                                tracked_dir_slug_source(path),
                                uuid,
                            )),
                            game_id: res.id,
                            path,
                            id: None,
                        }
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

            res = ensure_default_slot(conn, &res)?;

            res
        };

        if let Some(repository_root) = &self.repository_root {
            let full_game = self
                .get_game(res.id)
                .context("Error loading full game for metadata sync")?;
            crate::repository::sync_game_metadata(repository_root, &full_game)
                .context("Error writing game metadata")?;
        }

        Ok(res)
    }

    fn delete_game(&self, target_id: i32) -> Result<()> {
        use crate::schema::games::dsl;

        let conn = &mut self.pool.get().context("Error getting db connection")?;

        diesel::delete(dsl::games.filter(dsl::id.eq(target_id)))
            .execute(conn)
            .context("Error deleting game")?;

        Ok(())
    }
}

fn load_full_game(conn: &mut diesel::SqliteConnection, id: i32) -> Result<FullGame> {
    use crate::schema::games;
    use crate::schema::versions;

    let game: Game = games::dsl::games
        .filter(games::dsl::id.eq(id))
        .select(Game::as_select())
        .first(conn)
        .context("Error loading game")?;

    let dirs: Vec<TrackedDir> = TrackedDir::belonging_to(&game)
        .select(TrackedDir::as_select())
        .load(conn)?;

    let slots: Vec<Slot> = Slot::belonging_to(&game)
        .select(Slot::as_select())
        .order_by(crate::schema::slots::dsl::created_at.asc())
        .load(conn)?;

    let loaded_versions: Vec<Version> = Version::belonging_to(&slots)
        .select(Version::as_select())
        .order_by(versions::dsl::created_at.desc())
        .load(conn)?;

    let active_slot = slots
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

    Ok(FullGame {
        last_version: if let Some(slot) = active_slot.as_ref() {
            active_slot_versions
                .iter()
                .find(|v| Some(v.id) == slot.active_version_id)
                .cloned()
        } else {
            None
        },
        tracked_dirs: dirs,
        slots,
        active_slot,
        game,
        versions: active_slot_versions,
    })
}

fn load_full_slot(conn: &mut diesel::SqliteConnection, slot_id: i32) -> Result<FullSlot> {
    use crate::schema::games::dsl as games_dsl;
    use crate::schema::slots::dsl as slots_dsl;
    use crate::schema::versions::dsl as versions_dsl;

    let slot: Slot = slots_dsl::slots
        .filter(slots_dsl::id.eq(slot_id))
        .select(Slot::as_select())
        .first(conn)
        .context("Error loading slot")?;

    let game: Game = games_dsl::games
        .filter(games_dsl::id.eq(slot.game_id))
        .select(Game::as_select())
        .first(conn)
        .context("Error loading game for slot")?;

    let versions: Vec<Version> = versions_dsl::versions
        .filter(versions_dsl::slot_id.eq(slot.id))
        .order_by(versions_dsl::created_at.desc())
        .select(Version::as_select())
        .load(conn)
        .context("Error loading slot versions")?;

    Ok(FullSlot {
        slot,
        game,
        versions,
    })
}

fn load_full_slots_for_game(
    conn: &mut diesel::SqliteConnection,
    game_id: i32,
) -> Result<Vec<FullSlot>> {
    use crate::schema::slots::dsl as slots_dsl;

    let slot_ids: Vec<i32> = slots_dsl::slots
        .filter(slots_dsl::game_id.eq(game_id))
        .order_by(slots_dsl::created_at.asc())
        .select(slots_dsl::id)
        .load(conn)
        .context("Error loading slot ids for game")?;

    slot_ids
        .into_iter()
        .map(|slot_id| load_full_slot(conn, slot_id))
        .collect()
}

fn sync_game_metadata_if_needed(repo: &SqliteGameRepository, game_id: i32) -> Result<()> {
    if let Some(repository_root) = &repo.repository_root {
        let full_game = repo
            .get_game(game_id)
            .context("Error loading full game for metadata sync")?;
        crate::repository::sync_game_metadata(repository_root, &full_game)
            .context("Error writing game metadata")?;
    }

    Ok(())
}

fn enrich_game_form(
    conn: &mut diesel::SqliteConnection,
    form: GameForm<'_>,
) -> Result<PersistedGameForm> {
    use crate::schema::games::dsl as games_dsl;

    let existing = match form.id {
        Some(game_id) => games_dsl::games
            .filter(games_dsl::id.eq(game_id))
            .select(Game::as_select())
            .first(conn)
            .optional()
            .context("Error loading existing game for durable identity")?,
        None => None,
    };

    let uuid = existing
        .as_ref()
        .and_then(|game| game.uuid.clone())
        .or(form.uuid.clone())
        .filter(|value| !value.is_empty())
        .and_then(|value| Uuid::parse_str(&value).ok())
        .unwrap_or_else(Uuid::new_v4);

    let title = form.title.to_string();
    let title_normalized = crate::repository::normalize_display_name(&title);
    let folder_slug = crate::repository::build_folder_slug(
        crate::repository::RepositoryEntityKind::Game,
        &title,
        uuid,
    );

    Ok(PersistedGameForm {
        id: form.id,
        uuid: Some(uuid.to_string()),
        title,
        title_normalized: Some(title_normalized),
        folder_slug: Some(folder_slug),
        active_slot_id: form
            .active_slot_id
            .or(existing.as_ref().and_then(|game| game.active_slot_id)),
        icon_path: form.icon_path,
    })
}

fn ensure_default_slot(conn: &mut diesel::SqliteConnection, game: &Game) -> Result<Game> {
    use crate::schema::games::dsl as games_dsl;

    if game.active_slot_id.is_some() {
        return Ok(game.clone());
    }

    let slot_uuid = Uuid::new_v4();
    let slot_form = SlotForm {
        id: None,
        uuid: slot_uuid.to_string(),
        game_id: game.id,
        name: "Default Slot".to_string(),
        name_normalized: crate::repository::normalize_display_name("Default Slot"),
        folder_slug: crate::repository::build_folder_slug(
            crate::repository::RepositoryEntityKind::Slot,
            "Default Slot",
            slot_uuid,
        ),
        active_version_id: None,
        created_at: Some(Utc::now().naive_utc()),
        updated_at: Some(Utc::now().naive_utc()),
    };

    let slot: Slot = diesel::insert_into(crate::schema::slots::table)
        .values(&slot_form)
        .returning(Slot::as_returning())
        .get_result(conn)
        .context("Error creating default slot")?;

    diesel::update(games_dsl::games.filter(games_dsl::id.eq(game.id)))
        .set(games_dsl::active_slot_id.eq(slot.id))
        .returning(Game::as_returning())
        .get_result(conn)
        .context("Error updating game active slot")
}

fn tracked_dir_slug_source(path: &str) -> &str {
    Path::new(path)
        .file_name()
        .and_then(|segment| segment.to_str())
        .filter(|segment| !segment.is_empty())
        .unwrap_or("tracked-dir")
}
