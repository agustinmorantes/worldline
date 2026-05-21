pub mod games;
pub mod scan_roots;
pub mod versions;

use diesel::r2d2::{ConnectionManager, Pool};
use diesel::sqlite::SqliteConnection;
use std::path::PathBuf;

pub type DbPool = Pool<ConnectionManager<SqliteConnection>>;

pub struct SqliteGameRepository {
    pub pool: DbPool,
    pub repository_root: Option<PathBuf>,
}

impl SqliteGameRepository {
    pub fn new(pool: DbPool) -> Self {
        Self {
            pool,
            repository_root: None,
        }
    }

    pub fn with_repository_root(pool: DbPool, repository_root: PathBuf) -> Self {
        Self {
            pool,
            repository_root: Some(repository_root),
        }
    }
}

pub struct SqliteVersionRepository {
    pub pool: DbPool,
    pub repository_root: Option<PathBuf>,
}

impl SqliteVersionRepository {
    pub fn new(pool: DbPool) -> Self {
        Self {
            pool,
            repository_root: None,
        }
    }

    pub fn with_repository_root(pool: DbPool, repository_root: PathBuf) -> Self {
        Self {
            pool,
            repository_root: Some(repository_root),
        }
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
