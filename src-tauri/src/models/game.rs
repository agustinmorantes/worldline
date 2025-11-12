use diesel::{AsChangeset, Identifiable, Insertable, Queryable, Selectable};
use serde::{Deserialize, Serialize};

use crate::models::TrackedDir;
use crate::models::version::Version;

#[derive(Queryable, Selectable, Identifiable, Serialize, Clone, PartialEq, Debug)]
#[diesel(table_name = crate::schema::games)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct Game {
    pub id: i32,
    pub title: String,
    pub active_version_id: Option<i32>,
}

#[derive(
    Queryable, Insertable, Identifiable, AsChangeset, Deserialize, Clone, PartialEq, Debug,
)]
#[diesel(table_name = crate::schema::games)]
pub struct GameForm<'a> {
    pub id: Option<i32>,
    pub title: &'a str,
    pub active_version_id: Option<i32>,
}

#[derive(Serialize, Clone, PartialEq, Debug)]
pub struct FullGame {
    #[serde(flatten)]
    pub game: Game,
    pub tracked_dirs: Vec<TrackedDir>,
    pub versions: Vec<Version>,
    pub last_version: Option<Version>,
}
