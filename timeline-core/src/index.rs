//! SQLite projection of the history store plus device-local state (ADR 0004).
//! Rows are upserted by UUID from repository files, so integer ids stay
//! stable and the whole index can be rebuilt with `reindex`.

use crate::models::*;
use crate::schema::{games, scan_roots, slots, tracked_dirs, versions};
use crate::store::{GameRecord, normalize_display_name};
use anyhow::{Context, Result};
use diesel::SqliteConnection;
use diesel::prelude::*;
use diesel::r2d2::{ConnectionManager, Pool, PooledConnection};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

pub type DbPool = Pool<ConnectionManager<SqliteConnection>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pointer {
    pub game: Uuid,
    pub active: bool,
    pub slot: Uuid,
    pub version: Option<Uuid>,
}

#[derive(Clone)]
pub struct Index {
    pool: DbPool,
}

impl Index {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    fn conn(&self) -> Result<PooledConnection<ConnectionManager<SqliteConnection>>> {
        self.pool
            .get()
            .context("failed to get a database connection")
    }

    /// Projects one game from its files. Device-local columns (active
    /// pointers, icon) are kept; a tombstoned game is removed.
    pub fn upsert_game(&self, record: &GameRecord, retired: &HashSet<Uuid>) -> Result<()> {
        let conn = &mut self.conn()?;
        conn.transaction(|conn| upsert_game(conn, record, retired))
    }

    pub fn remove_game(&self, uuid: Uuid) -> Result<()> {
        let conn = &mut self.conn()?;
        diesel::delete(games::table.filter(games::uuid.eq(uuid.to_string()))).execute(conn)?;
        Ok(())
    }

    /// Rebuilds the projection of every game from the catalog.
    pub fn reindex(&self, catalog: &[GameRecord], retired: &HashSet<Uuid>) -> Result<()> {
        let conn = &mut self.conn()?;
        conn.transaction(|conn| {
            let live: Vec<String> = catalog
                .iter()
                .filter(|game| game.meta.deleted_at.is_none())
                .map(|game| game.meta.uuid.to_string())
                .collect();
            diesel::delete(games::table.filter(games::uuid.ne_all(&live))).execute(conn)?;
            for record in catalog {
                upsert_game(conn, record, retired)?;
            }
            Ok(())
        })
    }

    pub fn get_games(&self) -> Result<Vec<FullGame>> {
        let conn = &mut self.conn()?;
        let all: Vec<Game> = games::table
            .order_by(games::title_normalized)
            .select(Game::as_select())
            .load(conn)?;
        let dirs = TrackedDir::belonging_to(&all)
            .filter(tracked_dirs::retired_at.is_null())
            .select(TrackedDir::as_select())
            .load(conn)?;
        let all_slots = Slot::belonging_to(&all)
            .order_by((slots::created_at, slots::id))
            .select(Slot::as_select())
            .load(conn)?;
        let active_slot_ids: Vec<i32> = all.iter().filter_map(|g| g.active_slot_id).collect();
        let active_versions: Vec<Version> = versions::table
            .filter(versions::slot_id.eq_any(&active_slot_ids))
            .order_by((versions::created_at.desc(), versions::id.desc()))
            .select(Version::as_select())
            .load(conn)?;
        let mut versions_by_slot: HashMap<i32, Vec<Version>> = HashMap::new();
        for version in active_versions {
            versions_by_slot
                .entry(version.slot_id)
                .or_default()
                .push(version);
        }
        Ok(dirs
            .grouped_by(&all)
            .into_iter()
            .zip(all_slots.grouped_by(&all))
            .zip(all)
            .map(|((dirs, slots), game)| {
                let versions = game
                    .active_slot_id
                    .and_then(|id| versions_by_slot.remove(&id))
                    .unwrap_or_default();
                full_game(game, dirs, slots, versions)
            })
            .collect())
    }

    pub fn get_game(&self, id: i32) -> Result<FullGame> {
        let conn = &mut self.conn()?;
        let game: Game = games::table
            .find(id)
            .select(Game::as_select())
            .first(conn)
            .with_context(|| format!("game {id} not found"))?;
        let dirs = TrackedDir::belonging_to(&game)
            .filter(tracked_dirs::retired_at.is_null())
            .select(TrackedDir::as_select())
            .load(conn)?;
        let slots = Slot::belonging_to(&game)
            .order_by((slots::created_at, slots::id))
            .select(Slot::as_select())
            .load(conn)?;
        let versions = match game.active_slot_id {
            Some(slot_id) => slot_versions(conn, slot_id)?,
            None => Vec::new(),
        };
        Ok(full_game(game, dirs, slots, versions))
    }

    pub fn get_slot(&self, id: i32) -> Result<FullSlot> {
        let conn = &mut self.conn()?;
        let slot: Slot = slots::table
            .find(id)
            .select(Slot::as_select())
            .first(conn)
            .with_context(|| format!("slot {id} not found"))?;
        let versions = slot_versions(conn, id)?;
        Ok(FullSlot { slot, versions })
    }

    pub fn get_version(&self, id: i32) -> Result<FullVersion> {
        let conn = &mut self.conn()?;
        let (version, slot, game) = versions::table
            .inner_join(slots::table.inner_join(games::table))
            .filter(versions::id.eq(id))
            .select((Version::as_select(), Slot::as_select(), Game::as_select()))
            .first(conn)
            .with_context(|| format!("version {id} not found"))?;
        Ok(FullVersion {
            version,
            game,
            slot,
        })
    }

    pub fn game_id(&self, uuid: Uuid) -> Result<Option<i32>> {
        Ok(games::table
            .filter(games::uuid.eq(uuid.to_string()))
            .select(games::id)
            .first(&mut self.conn()?)
            .optional()?)
    }

    pub fn version_id(&self, uuid: Uuid) -> Result<Option<i32>> {
        Ok(versions::table
            .filter(versions::uuid.eq(uuid.to_string()))
            .select(versions::id)
            .first(&mut self.conn()?)
            .optional()?)
    }

    pub fn slot_id(&self, uuid: Uuid) -> Result<Option<i32>> {
        Ok(slots::table
            .filter(slots::uuid.eq(uuid.to_string()))
            .select(slots::id)
            .first(&mut self.conn()?)
            .optional()?)
    }

    pub fn set_active_slot(&self, game_id: i32, slot_id: i32) -> Result<()> {
        let conn = &mut self.conn()?;
        let belongs: i64 = slots::table
            .filter(slots::id.eq(slot_id).and(slots::game_id.eq(game_id)))
            .count()
            .get_result(conn)?;
        anyhow::ensure!(
            belongs == 1,
            "slot {slot_id} does not belong to game {game_id}"
        );
        diesel::update(games::table.find(game_id))
            .set(games::active_slot_id.eq(slot_id))
            .execute(conn)?;
        Ok(())
    }

    pub fn set_active_version(&self, slot_id: i32, version_id: Option<i32>) -> Result<()> {
        let conn = &mut self.conn()?;
        diesel::update(slots::table.find(slot_id))
            .set(slots::active_version_id.eq(version_id))
            .execute(conn)?;
        Ok(())
    }

    /// Device-local pointers of every slot: (game, is active slot, slot, active version).
    pub fn pointers(&self) -> Result<Vec<Pointer>> {
        type Row = (String, Option<i32>, i32, String, Option<String>);
        let rows: Vec<Row> = slots::table
            .inner_join(games::table)
            .left_join(versions::table.on(slots::active_version_id.eq(versions::id.nullable())))
            .select((
                games::uuid,
                games::active_slot_id,
                slots::id,
                slots::uuid,
                versions::uuid.nullable(),
            ))
            .load(&mut self.conn()?)?;
        rows.into_iter()
            .map(|(game, active_slot, slot_id, slot, version)| {
                Ok(Pointer {
                    game: Uuid::parse_str(&game)?,
                    active: active_slot == Some(slot_id),
                    slot: Uuid::parse_str(&slot)?,
                    version: version.map(|v| Uuid::parse_str(&v)).transpose()?,
                })
            })
            .collect()
    }

    pub fn set_icon(&self, game_id: i32, icon_path: Option<&str>) -> Result<()> {
        diesel::update(games::table.find(game_id))
            .set(games::icon_path.eq(icon_path))
            .execute(&mut self.conn()?)?;
        Ok(())
    }

    pub fn retired_version_uuids(&self) -> Result<Vec<String>> {
        Ok(versions::table
            .filter(versions::retired.eq(true))
            .order_by(versions::uuid)
            .select(versions::uuid)
            .load(&mut self.conn()?)?)
    }

    pub fn retired_tracked_dirs(&self, game_id: i32) -> Result<Vec<TrackedDir>> {
        Ok(tracked_dirs::table
            .filter(tracked_dirs::game_id.eq(game_id))
            .filter(tracked_dirs::retired_at.is_not_null())
            .select(TrackedDir::as_select())
            .load(&mut self.conn()?)?)
    }

    /// Every tracked path of every game, retired ones included.
    pub fn tracked_paths(&self) -> Result<Vec<(i32, String)>> {
        Ok(tracked_dirs::table
            .select((tracked_dirs::game_id, tracked_dirs::path))
            .load(&mut self.conn()?)?)
    }

    pub fn get_scan_roots(&self) -> Result<Vec<String>> {
        Ok(scan_roots::table
            .order_by(scan_roots::id)
            .select(scan_roots::path)
            .load(&mut self.conn()?)?)
    }

    pub fn add_scan_root(&self, path: &str) -> Result<()> {
        diesel::insert_into(scan_roots::table)
            .values(scan_roots::path.eq(path))
            .on_conflict_do_nothing()
            .execute(&mut self.conn()?)?;
        Ok(())
    }

    pub fn remove_scan_root(&self, path: &str) -> Result<()> {
        diesel::delete(scan_roots::table.filter(scan_roots::path.eq(path)))
            .execute(&mut self.conn()?)?;
        Ok(())
    }
}

fn slot_versions(conn: &mut SqliteConnection, slot_id: i32) -> Result<Vec<Version>> {
    Ok(versions::table
        .filter(versions::slot_id.eq(slot_id))
        .order_by((versions::created_at.desc(), versions::id.desc()))
        .select(Version::as_select())
        .load(conn)?)
}

fn full_game(
    game: Game,
    tracked_dirs: Vec<TrackedDir>,
    slots: Vec<Slot>,
    versions: Vec<Version>,
) -> FullGame {
    let active_slot = slots
        .iter()
        .find(|s| Some(s.id) == game.active_slot_id)
        .cloned();
    let last_version = active_slot
        .as_ref()
        .and_then(|slot| {
            versions
                .iter()
                .find(|v| Some(v.id) == slot.active_version_id)
        })
        .cloned();
    FullGame {
        game,
        tracked_dirs,
        slots,
        active_slot,
        versions,
        last_version,
    }
}

fn upsert_game(
    conn: &mut SqliteConnection,
    record: &GameRecord,
    retired: &HashSet<Uuid>,
) -> Result<()> {
    let meta = &record.meta;
    if meta.deleted_at.is_some() {
        diesel::delete(games::table.filter(games::uuid.eq(meta.uuid.to_string()))).execute(conn)?;
        return Ok(());
    }
    let game_id: i32 = diesel::insert_into(games::table)
        .values((
            games::uuid.eq(meta.uuid.to_string()),
            games::title.eq(&meta.title),
            games::title_normalized.eq(normalize_display_name(&meta.title)),
            games::folder_slug.eq(&meta.folder_slug),
            games::created_at.eq(meta.created_at.naive_utc()),
            games::updated_at.eq(meta.updated_at.naive_utc()),
        ))
        .on_conflict(games::uuid)
        .do_update()
        .set((
            games::title.eq(&meta.title),
            games::title_normalized.eq(normalize_display_name(&meta.title)),
            games::folder_slug.eq(&meta.folder_slug),
            games::updated_at.eq(meta.updated_at.naive_utc()),
        ))
        .returning(games::id)
        .get_result(conn)?;

    let mut dir_uuids = Vec::new();
    for dir in &record.tracked_dirs {
        let path = crate::path_placeholders::expand_path_placeholders(&dir.path);
        let exclusions = serde_json::to_string(&dir.exclusions)?;
        let retired_at = dir.retired_at.map(|at| at.naive_utc());
        diesel::insert_into(tracked_dirs::table)
            .values((
                tracked_dirs::uuid.eq(dir.uuid.to_string()),
                tracked_dirs::game_id.eq(game_id),
                tracked_dirs::folder_slug.eq(&dir.folder_slug),
                tracked_dirs::path.eq(&path),
                tracked_dirs::label.eq(&dir.label),
                tracked_dirs::exclusions.eq(&exclusions),
                tracked_dirs::retired_at.eq(retired_at),
                tracked_dirs::created_at.eq(dir.created_at.naive_utc()),
                tracked_dirs::updated_at.eq(dir.updated_at.naive_utc()),
            ))
            .on_conflict(tracked_dirs::uuid)
            .do_update()
            .set((
                tracked_dirs::path.eq(&path),
                tracked_dirs::label.eq(&dir.label),
                tracked_dirs::exclusions.eq(&exclusions),
                tracked_dirs::retired_at.eq(retired_at),
                tracked_dirs::updated_at.eq(dir.updated_at.naive_utc()),
            ))
            .execute(conn)?;
        dir_uuids.push(dir.uuid.to_string());
    }
    diesel::delete(
        tracked_dirs::table.filter(
            tracked_dirs::game_id
                .eq(game_id)
                .and(tracked_dirs::uuid.ne_all(&dir_uuids)),
        ),
    )
    .execute(conn)?;

    let mut slot_uuids = Vec::new();
    for slot in record
        .slots
        .iter()
        .filter(|slot| slot.meta.deleted_at.is_none())
    {
        let meta = &slot.meta;
        let slot_id: i32 = diesel::insert_into(slots::table)
            .values((
                slots::uuid.eq(meta.uuid.to_string()),
                slots::game_id.eq(game_id),
                slots::name.eq(&meta.name),
                slots::name_normalized.eq(normalize_display_name(&meta.name)),
                slots::folder_slug.eq(&meta.folder_slug),
                slots::created_at.eq(meta.created_at.naive_utc()),
                slots::updated_at.eq(meta.updated_at.naive_utc()),
            ))
            .on_conflict(slots::uuid)
            .do_update()
            .set((
                slots::name.eq(&meta.name),
                slots::name_normalized.eq(normalize_display_name(&meta.name)),
                slots::updated_at.eq(meta.updated_at.naive_utc()),
            ))
            .returning(slots::id)
            .get_result(conn)?;
        slot_uuids.push(meta.uuid.to_string());

        let mut ids: HashMap<Uuid, i32> = HashMap::new();
        let mut pending: Vec<_> = slot.versions.iter().collect();
        let mut break_cycles = false;
        while !pending.is_empty() {
            let before = pending.len();
            let mut deferred = Vec::new();
            for version in pending {
                let parent_id = match version.parent_uuid {
                    None => None,
                    Some(parent) if ids.contains_key(&parent) => ids.get(&parent).copied(),
                    // Parent not in this slot's files: link to it if indexed, else leave it open.
                    Some(parent) if !slot.versions.iter().any(|v| v.uuid == parent) => {
                        versions::table
                            .filter(versions::uuid.eq(parent.to_string()))
                            .select(versions::id)
                            .first(conn)
                            .optional()?
                    }
                    Some(_) if break_cycles => None,
                    Some(_) => {
                        deferred.push(version);
                        continue;
                    }
                };
                let is_retired = retired.contains(&version.uuid);
                let id: i32 = diesel::insert_into(versions::table)
                    .values((
                        versions::uuid.eq(version.uuid.to_string()),
                        versions::slot_id.eq(slot_id),
                        versions::parent_id.eq(parent_id),
                        versions::version_number.eq(version.version_number),
                        versions::folder_slug.eq(&version.folder_slug),
                        versions::reason.eq(version.reason.as_str()),
                        versions::tag.eq(&version.tag),
                        versions::retired.eq(is_retired),
                        versions::created_at.eq(version.created_at.naive_utc()),
                    ))
                    .on_conflict(versions::uuid)
                    .do_update()
                    .set((
                        versions::parent_id.eq(parent_id),
                        versions::tag.eq(&version.tag),
                        versions::retired.eq(is_retired),
                    ))
                    .returning(versions::id)
                    .get_result(conn)?;
                ids.insert(version.uuid, id);
            }
            // A parent cycle can only come from corrupt files; index the rest as roots.
            break_cycles = deferred.len() == before;
            pending = deferred;
        }
        let known: Vec<String> = slot.versions.iter().map(|v| v.uuid.to_string()).collect();
        diesel::delete(
            versions::table.filter(
                versions::slot_id
                    .eq(slot_id)
                    .and(versions::uuid.ne_all(&known)),
            ),
        )
        .execute(conn)?;
    }
    diesel::delete(
        slots::table.filter(
            slots::game_id
                .eq(game_id)
                .and(slots::uuid.ne_all(&slot_uuids)),
        ),
    )
    .execute(conn)?;

    // A game always has an active slot on this device; default to the oldest.
    let active: Option<i32> = games::table
        .find(game_id)
        .select(games::active_slot_id)
        .first(conn)?;
    if active.is_none() {
        let oldest: Option<i32> = slots::table
            .filter(slots::game_id.eq(game_id))
            .order_by((slots::created_at, slots::id))
            .select(slots::id)
            .first(conn)
            .optional()?;
        diesel::update(games::table.find(game_id))
            .set(games::active_slot_id.eq(oldest))
            .execute(conn)?;
    }
    Ok(())
}
