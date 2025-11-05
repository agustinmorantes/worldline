use diesel::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Queryable, Selectable, Serialize, Clone, Debug)]
#[diesel(table_name = crate::schema::games)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct Game {
    pub id: i32,
    pub title: String,
}

#[derive(Insertable, Deserialize, Clone, Debug)]
#[diesel(table_name = crate::schema::games)]
pub struct NewGame<'a> {
    pub title: &'a str
}
