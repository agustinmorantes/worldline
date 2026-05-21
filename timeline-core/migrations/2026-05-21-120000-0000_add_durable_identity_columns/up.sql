ALTER TABLE games ADD COLUMN uuid TEXT;
ALTER TABLE games ADD COLUMN title_normalized TEXT;
ALTER TABLE games ADD COLUMN folder_slug TEXT;

UPDATE games
SET uuid = lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-' || lower(hex(randomblob(2))) || '-' || lower(hex(randomblob(2))) || '-' || lower(hex(randomblob(6)))
WHERE uuid IS NULL;

UPDATE games
SET title_normalized = lower(trim(title))
WHERE title_normalized IS NULL;

UPDATE games
SET folder_slug = lower(replace(trim(title), ' ', '-')) || '__g_' || substr(replace(uuid, '-', ''), 1, 8)
WHERE folder_slug IS NULL;

CREATE UNIQUE INDEX idx_games_uuid ON games(uuid);
CREATE UNIQUE INDEX idx_games_title_normalized ON games(title_normalized);
CREATE UNIQUE INDEX idx_games_folder_slug ON games(folder_slug);

ALTER TABLE tracked_dirs ADD COLUMN uuid TEXT;
ALTER TABLE tracked_dirs ADD COLUMN folder_slug TEXT;

UPDATE tracked_dirs
SET uuid = lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-' || lower(hex(randomblob(2))) || '-' || lower(hex(randomblob(2))) || '-' || lower(hex(randomblob(6)))
WHERE uuid IS NULL;

UPDATE tracked_dirs
SET folder_slug = 'tracked-dir-' || id || '__d_' || substr(replace(uuid, '-', ''), 1, 8)
WHERE folder_slug IS NULL;

CREATE UNIQUE INDEX idx_tracked_dirs_uuid ON tracked_dirs(uuid);
CREATE UNIQUE INDEX idx_tracked_dirs_folder_slug ON tracked_dirs(folder_slug);

ALTER TABLE versions ADD COLUMN uuid TEXT;
ALTER TABLE versions ADD COLUMN folder_slug TEXT;

UPDATE versions
SET uuid = lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-' || lower(hex(randomblob(2))) || '-' || lower(hex(randomblob(2))) || '-' || lower(hex(randomblob(6)))
WHERE uuid IS NULL;

UPDATE versions
SET folder_slug = 'v' || printf('%03d', version_number) || '__v_' || substr(replace(uuid, '-', ''), 1, 8)
WHERE folder_slug IS NULL;

CREATE UNIQUE INDEX idx_versions_uuid ON versions(uuid);
CREATE UNIQUE INDEX idx_versions_folder_slug ON versions(folder_slug);
