-- Reconstruct parent_id for versions created before parent_id tracking was active.
-- Links each version to the previous version_number within the same game (linear chain).
UPDATE versions
SET parent_id = (
  SELECT v2.id
  FROM versions v2
  WHERE v2.game_id = versions.game_id
    AND v2.version_number = versions.version_number - 1
)
WHERE parent_id IS NULL
  AND version_number > 1;

-- Fix games where active_version_id references a non-existent version (data corruption).
UPDATE games
SET active_version_id = (
  SELECT id FROM versions
  WHERE game_id = games.id
  ORDER BY version_number DESC
  LIMIT 1
)
WHERE active_version_id IS NOT NULL
  AND NOT EXISTS (
    SELECT 1 FROM versions WHERE id = games.active_version_id
  );
