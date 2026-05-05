pub mod games;
pub mod versions;

use diesel::sqlite::SqliteConnection;
use diesel::r2d2::{ConnectionManager, Pool};

pub type DbPool = Pool<ConnectionManager<SqliteConnection>>;

pub struct SqliteGameRepository {
    pub pool: DbPool,
}

impl SqliteGameRepository {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

pub struct SqliteVersionRepository {
    pub pool: DbPool,
}

impl SqliteVersionRepository {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}
