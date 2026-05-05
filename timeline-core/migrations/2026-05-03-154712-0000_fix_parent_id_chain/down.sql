-- Revert: clear parent_id for all versions (cannot recover original corruption)
UPDATE versions SET parent_id = NULL WHERE version_number > 1;
