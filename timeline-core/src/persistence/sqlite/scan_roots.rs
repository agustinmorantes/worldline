use super::SqliteScanRootsRepository;
use crate::models::*;
use crate::persistence::ScanRootsRepository;
use anyhow::{Context, Result};
use diesel::prelude::*;

impl ScanRootsRepository for SqliteScanRootsRepository {
    fn get_scan_roots(&self) -> Result<Vec<String>> {
        use crate::schema::scan_roots::dsl::*;

        let conn = &mut self.pool.get().context("Error getting db connection")?;

        let results = scan_roots
            .select(path)
            .load::<String>(conn)
            .context("Error loading scan roots")?;

        Ok(results)
    }

    fn add_scan_root(&self, target_path: &str) -> Result<()> {
        use crate::schema::scan_roots;

        let conn = &mut self.pool.get().context("Error getting db connection")?;

        let form = ScanRootForm {
            path: target_path.to_string(),
        };

        diesel::insert_into(scan_roots::table)
            .values(&form)
            .on_conflict(scan_roots::path)
            .do_nothing()
            .execute(conn)
            .context("Error inserting scan root")?;

        Ok(())
    }

    fn remove_scan_root(&self, target_path: &str) -> Result<()> {
        use crate::schema::scan_roots::dsl::*;

        let conn = &mut self.pool.get().context("Error getting db connection")?;

        diesel::delete(scan_roots.filter(path.eq(target_path)))
            .execute(conn)
            .context("Error deleting scan root")?;

        Ok(())
    }
}
