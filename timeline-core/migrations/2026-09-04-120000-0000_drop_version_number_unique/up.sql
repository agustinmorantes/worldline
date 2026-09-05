-- version_number is a display hint, not identity. Two devices syncing offline work
-- may legitimately produce the same number within a slot; the UUID disambiguates.
DROP INDEX IF EXISTS idx_versions_slot_version;
CREATE INDEX idx_versions_slot_version ON versions(slot_id, version_number);
