pub mod games;
pub mod scan_roots;
pub mod versions;

use diesel::r2d2::{ConnectionManager, Pool};
use diesel::sqlite::SqliteConnection;

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

pub struct SqliteScanRootsRepository {
    pub pool: DbPool,
}

impl SqliteScanRootsRepository {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}
