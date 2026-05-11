use diesel::{AsChangeset, Identifiable, Insertable, Queryable, Selectable};
use serde::{Deserialize, Serialize};

#[derive(Queryable, Selectable, Identifiable, Serialize, Clone, PartialEq, Debug)]
#[diesel(table_name = crate::schema::scan_roots)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct ScanRoot {
    pub id: i32,
    pub path: String,
}

#[derive(Insertable, AsChangeset, Deserialize, Clone, PartialEq, Debug)]
#[diesel(table_name = crate::schema::scan_roots)]
pub struct ScanRootForm {
    pub path: String,
}
