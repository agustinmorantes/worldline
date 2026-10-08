-- Index over the history store (ADR 0001, ADR 0004). Everything except the
-- device-local columns (active pointers) and scan_roots can be rebuilt from
-- the repository files; icon_path is the local store's copy of the game icon
-- (ADR 0005). Only UUIDs are unique: title, slot-name and path uniqueness is
-- enforced on local edits, so synced data never fails an insert.
CREATE TABLE games (
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    uuid TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL,
    title_normalized TEXT NOT NULL,
    folder_slug TEXT NOT NULL,
    active_slot_id INTEGER REFERENCES slots(id) ON DELETE SET NULL,
    icon_path TEXT,
    accent_hue INTEGER NOT NULL,
    accent_hue_source TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL
);

CREATE TABLE slots (
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    uuid TEXT NOT NULL UNIQUE,
    game_id INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    name_normalized TEXT NOT NULL,
    folder_slug TEXT NOT NULL,
    active_version_id INTEGER REFERENCES versions(id) ON DELETE SET NULL,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL
);
CREATE INDEX slots_game_id ON slots(game_id);

CREATE TABLE tracked_dirs (
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    uuid TEXT NOT NULL UNIQUE,
    game_id INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    folder_slug TEXT NOT NULL,
    path TEXT NOT NULL,
    label TEXT,
    exclusions TEXT NOT NULL DEFAULT '[]',
    retired_at TIMESTAMP,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL
);
CREATE INDEX tracked_dirs_game_id ON tracked_dirs(game_id);

CREATE TABLE versions (
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    uuid TEXT NOT NULL UNIQUE,
    slot_id INTEGER NOT NULL REFERENCES slots(id) ON DELETE CASCADE,
    parent_id INTEGER REFERENCES versions(id) ON DELETE SET NULL,
    version_number INTEGER NOT NULL,
    folder_slug TEXT NOT NULL,
    reason TEXT NOT NULL,
    tag TEXT,
    retired BOOLEAN NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL,
    origin_device TEXT
);
CREATE INDEX versions_slot_id ON versions(slot_id);

CREATE TABLE scan_roots (
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    path TEXT NOT NULL COLLATE NOCASE UNIQUE
);
