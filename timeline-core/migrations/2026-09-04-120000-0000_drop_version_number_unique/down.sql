DROP INDEX IF EXISTS idx_versions_slot_version;
CREATE UNIQUE INDEX idx_versions_slot_version ON versions(slot_id, version_number);
