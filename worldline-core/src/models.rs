//! Index rows as the GUI and CLI see them. Integer ids are local to this
//! device's index; UUIDs are the shared identity.

use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Queryable, Selectable, Identifiable, Serialize, Clone, PartialEq, Debug)]
#[diesel(table_name = crate::schema::games)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Game {
    pub id: i32,
    pub uuid: String,
    pub title: String,
    pub title_normalized: String,
    pub folder_slug: String,
    pub active_slot_id: Option<i32>,
    /// The local copy of the game's icon, once it has arrived.
    pub icon_path: Option<String>,
    /// The accent hue to show (0–359): the stored one, or the UUID fallback.
    pub accent_hue: i32,
    /// `auto` (from the icon) or `user`.
    pub accent_hue_source: String,
}

/// What a caller may set when creating or editing a game.
#[derive(Deserialize, Clone, PartialEq, Debug)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct GameForm {
    pub id: Option<i32>,
    pub title: String,
}

#[derive(Queryable, Selectable, Identifiable, Associations, Serialize, Clone, PartialEq, Debug)]
#[diesel(belongs_to(Game))]
#[diesel(table_name = crate::schema::slots)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Slot {
    pub id: i32,
    pub uuid: String,
    pub game_id: i32,
    pub name: String,
    pub name_normalized: String,
    pub folder_slug: String,
    pub active_version_id: Option<i32>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Queryable, Selectable, Identifiable, Associations, Serialize, Clone, PartialEq, Debug)]
#[diesel(belongs_to(Game))]
#[diesel(table_name = crate::schema::tracked_dirs)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TrackedDir {
    pub id: i32,
    pub uuid: String,
    pub game_id: i32,
    pub folder_slug: String,
    pub path: String,
    pub label: Option<String>,
    /// JSON array of glob patterns the user does not want versioned.
    pub exclusions: String,
    pub retired_at: Option<NaiveDateTime>,
}

#[derive(Queryable, Selectable, Identifiable, Associations, Serialize, Clone, PartialEq, Debug)]
#[diesel(belongs_to(Slot))]
#[diesel(table_name = crate::schema::versions)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Version {
    pub id: i32,
    pub uuid: String,
    pub slot_id: i32,
    pub parent_id: Option<i32>,
    pub version_number: i32,
    pub folder_slug: String,
    pub reason: String,
    pub tag: Option<String>,
    /// Pruned: the graph node remains but its content is not restorable.
    pub retired: bool,
    pub created_at: NaiveDateTime,
    /// UUID of the device that created it, when known.
    pub origin_device: Option<String>,
}

/// Every slot's size and last activity, for picking a playthrough.
#[derive(Serialize, Clone, PartialEq, Debug)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SlotSummary {
    pub slot_id: i32,
    /// Retired versions included.
    pub version_count: i32,
    pub last_created_at: Option<NaiveDateTime>,
}

#[derive(Serialize, Clone, PartialEq, Debug)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct FullGame {
    #[serde(flatten)]
    pub game: Game,
    /// Live (non-retired) tracked directories.
    pub tracked_dirs: Vec<TrackedDir>,
    pub slots: Vec<Slot>,
    /// One per slot, in the order of `slots`.
    pub slot_summaries: Vec<SlotSummary>,
    pub active_slot: Option<Slot>,
    /// Versions of the active slot, newest first.
    pub versions: Vec<Version>,
    /// The active slot's active version.
    pub last_version: Option<Version>,
}

#[derive(Serialize, Clone, PartialEq, Debug)]
pub struct FullSlot {
    #[serde(flatten)]
    pub slot: Slot,
    pub versions: Vec<Version>,
}

#[derive(Serialize, Clone, PartialEq, Debug)]
pub struct FullVersion {
    #[serde(flatten)]
    pub version: Version,
    pub game: Game,
    pub slot: Slot,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct MonitoringSettings {
    pub version_cooldown_minutes: u32,
    pub snapshot_debounce_seconds: f32,
}

impl Default for MonitoringSettings {
    fn default() -> Self {
        Self {
            version_cooldown_minutes: 15,
            snapshot_debounce_seconds: 5.0,
        }
    }
}
