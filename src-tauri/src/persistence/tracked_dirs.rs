use crate::models::TrackedDir;
use crate::DbPool;
use diesel::prelude::*;

pub fn get_tracked_dirs(db: DbPool) -> Vec<TrackedDir> {
    let conn = &mut db
        .get()
        .expect("Error getting db connection");

    use crate::schema::tracked_dirs::dsl::*;
    let Ok(dirs) = tracked_dirs
        .select(TrackedDir::as_select())
        .load(conn)
    else {
        println!("Error loading tracked dirs");
        return vec![];
    };

    dirs
}
