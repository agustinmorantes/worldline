-- 1. Recreate versions: remove game_id, make slot_id NOT NULL, add reason and content_hash.
--    For rows where slot_id is NULL, fall back to the first slot for that game (by created_at).

CREATE TABLE versions_new (
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    uuid TEXT,
    folder_slug TEXT,
    version_number INTEGER NOT NULL,
    slot_id INTEGER NOT NULL REFERENCES slots(id),
    tag TEXT,
    reason TEXT NOT NULL DEFAULT 'watcher',
    content_hash TEXT,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    parent_id INTEGER REFERENCES versions_new(id)
);

INSERT INTO versions_new (id, uuid, folder_slug, version_number, slot_id, tag, reason, content_hash, created_at, parent_id)
SELECT
    v.id,
    v.uuid,
    v.folder_slug,
    v.version_number,
    COALESCE(
        v.slot_id,
        (SELECT s.id FROM slots s WHERE s.game_id = v.game_id ORDER BY s.created_at ASC LIMIT 1)
    ) AS slot_id,
    v.tag,
    'watcher' AS reason,
    NULL AS content_hash,
    v.created_at,
    v.parent_id
FROM versions v;

DROP TABLE versions;
ALTER TABLE versions_new RENAME TO versions;

CREATE UNIQUE INDEX idx_versions_uuid ON versions(uuid);
CREATE UNIQUE INDEX idx_versions_folder_slug ON versions(folder_slug);
CREATE UNIQUE INDEX idx_versions_slot_version ON versions(slot_id, version_number);
CREATE INDEX idx_versions_slot_content_hash ON versions(slot_id, content_hash);

-- 2. Recreate games: remove active_version_id, add created_at and updated_at.

CREATE TABLE games_new (
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    uuid TEXT,
    title TEXT NOT NULL,
    title_normalized TEXT,
    folder_slug TEXT,
    active_slot_id INTEGER REFERENCES slots(id),
    icon_path TEXT,
    created_at TIMESTAMP NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S', 'now')),
    updated_at TIMESTAMP NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S', 'now'))
);

INSERT INTO games_new (id, uuid, title, title_normalized, folder_slug, active_slot_id, icon_path, created_at, updated_at)
SELECT
    id,
    uuid,
    title,
    title_normalized,
    folder_slug,
    active_slot_id,
    icon_path,
    strftime('%Y-%m-%dT%H:%M:%S', 'now'),
    strftime('%Y-%m-%dT%H:%M:%S', 'now')
FROM games;

DROP TABLE games;
ALTER TABLE games_new RENAME TO games;

CREATE UNIQUE INDEX idx_games_uuid ON games(uuid);
CREATE UNIQUE INDEX idx_games_title_normalized ON games(title_normalized);
CREATE UNIQUE INDEX idx_games_folder_slug ON games(folder_slug);

-- 3. Add new columns to tracked_dirs.

ALTER TABLE tracked_dirs ADD COLUMN label TEXT;
ALTER TABLE tracked_dirs ADD COLUMN exclusions TEXT NOT NULL DEFAULT '[]';
ALTER TABLE tracked_dirs ADD COLUMN retired_at TIMESTAMP;
ALTER TABLE tracked_dirs ADD COLUMN created_at TIMESTAMP NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S', 'now'));
ALTER TABLE tracked_dirs ADD COLUMN updated_at TIMESTAMP NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S', 'now'));
