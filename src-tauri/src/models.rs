use diesel::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Queryable, Selectable, Identifiable, Serialize, Clone, PartialEq, Debug)]
#[diesel(table_name = crate::schema::games)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct Game {
    pub id: i32,
    pub title: String,
}

#[derive(
    Queryable, Insertable, Identifiable, AsChangeset, Deserialize, Clone, PartialEq, Debug,
)]
#[diesel(table_name = crate::schema::games)]
pub struct GameForm<'a> {
    pub id: Option<i32>,
    pub title: &'a str,
}

#[derive(Queryable, Selectable, Identifiable, Associations, Serialize, Clone, PartialEq, Debug)]
#[diesel(belongs_to(Game))]
#[diesel(table_name = crate::schema::tracked_dirs)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct TrackedDir {
    pub id: i32,
    pub game_id: i32,
    pub path: String,
}

#[derive(
    Queryable, Insertable, Identifiable, AsChangeset, Deserialize, Clone, PartialEq, Debug,
)]
#[diesel(table_name = crate::schema::tracked_dirs)]
pub struct TrackedDirForm<'a> {
    pub id: Option<i32>,
    pub game_id: i32,
    pub path: &'a str,
}

#[derive(Serialize, Clone, PartialEq, Debug)]
pub struct FullGame {
    #[serde(flatten)]
    pub game: Game,
    pub tracked_dirs: Vec<TrackedDir>,
}
