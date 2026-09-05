use crate::models::{Game, Version};
use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Queryable, Selectable, Identifiable, Associations, Serialize, Clone, PartialEq, Debug)]
#[diesel(belongs_to(Game))]
#[diesel(table_name = crate::schema::slots)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
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

#[derive(
    Queryable, Insertable, Identifiable, AsChangeset, Deserialize, Clone, PartialEq, Debug,
)]
#[diesel(table_name = crate::schema::slots)]
pub struct SlotForm {
    pub id: Option<i32>,
    pub uuid: String,
    pub game_id: i32,
    pub name: String,
    pub name_normalized: String,
    pub folder_slug: String,
    pub active_version_id: Option<i32>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
}

#[derive(Serialize, Clone, PartialEq, Debug)]
pub struct FullSlot {
    #[serde(flatten)]
    pub slot: Slot,
    pub game: Game,
    pub versions: Vec<Version>,
}
