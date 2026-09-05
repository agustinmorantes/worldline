use crate::models::Game;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Queryable, Selectable, Identifiable, Associations, Serialize, Clone, PartialEq, Debug)]
#[diesel(belongs_to(Game))]
#[diesel(table_name = crate::schema::tracked_dirs)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct TrackedDir {
    pub id: i32,
    pub uuid: Option<String>,
    pub folder_slug: Option<String>,
    pub game_id: i32,
    pub path: String,
    /// JSON array of glob patterns the user does not want versioned.
    pub exclusions: String,
}

#[derive(
    Queryable, Insertable, Identifiable, AsChangeset, Deserialize, Clone, PartialEq, Debug,
)]
#[diesel(table_name = crate::schema::tracked_dirs)]
pub struct TrackedDirForm<'a> {
    pub id: Option<i32>,
    pub uuid: Option<String>,
    pub folder_slug: Option<String>,
    pub game_id: i32,
    pub path: &'a str,
}

#[derive(Serialize, Clone, PartialEq, Debug)]
pub struct FullTrackedDir {
    #[serde(flatten)]
    pub tracked_dir: TrackedDir,
    pub game: Game,
}
