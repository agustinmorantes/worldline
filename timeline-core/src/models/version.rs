use crate::models::{Game, Slot};
use chrono::NaiveDateTime;
use diesel::{AsChangeset, Associations, Identifiable, Insertable, Queryable, Selectable};
use serde::{Deserialize, Serialize};

#[derive(Queryable, Selectable, Identifiable, Associations, Serialize, Clone, PartialEq, Debug)]
#[diesel(belongs_to(Slot))]
#[diesel(table_name = crate::schema::versions)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct Version {
    pub id: i32,
    pub uuid: Option<String>,
    pub folder_slug: Option<String>,
    pub version_number: i32,
    pub slot_id: i32,
    pub tag: Option<String>,
    pub reason: String,
    pub created_at: NaiveDateTime,
    pub parent_id: Option<i32>,
}

#[derive(
    Queryable, Insertable, Identifiable, AsChangeset, Deserialize, Clone, PartialEq, Debug,
)]
#[diesel(table_name = crate::schema::versions)]
pub struct VersionForm<'a> {
    pub id: Option<i32>,
    pub uuid: Option<String>,
    pub folder_slug: Option<String>,
    pub slot_id: i32,
    pub version_number: i32,
    pub tag: Option<&'a str>,
    pub reason: String,
    pub created_at: Option<NaiveDateTime>,
    pub parent_id: Option<i32>,
}

#[derive(Serialize, Clone, PartialEq, Debug)]
pub struct FullVersion {
    #[serde(flatten)]
    pub version: Version,
    pub game: Game,
    pub slot: Slot,
}
