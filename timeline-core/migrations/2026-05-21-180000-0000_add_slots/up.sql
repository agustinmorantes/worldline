CREATE TABLE slots (
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    uuid TEXT NOT NULL UNIQUE,
    game_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    name_normalized TEXT NOT NULL,
    folder_slug TEXT NOT NULL UNIQUE,
    active_version_id INTEGER,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (game_id) REFERENCES games(id) ON DELETE CASCADE,
    FOREIGN KEY (active_version_id) REFERENCES versions(id),
    UNIQUE (game_id, name_normalized)
);

CREATE INDEX idx_slots_game_id ON slots(game_id);

ALTER TABLE games ADD COLUMN active_slot_id INTEGER REFERENCES slots(id);

CREATE TABLE versions_new (
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    uuid TEXT,
    folder_slug TEXT,
    version_number INTEGER NOT NULL,
    game_id INTEGER NOT NULL,
    slot_id INTEGER REFERENCES slots(id),
    tag TEXT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP NOT NULL,
    parent_id INTEGER REFERENCES versions(id),
    FOREIGN KEY (game_id) REFERENCES games(id) ON DELETE CASCADE,
    UNIQUE (slot_id, version_number)
);

INSERT INTO versions_new (id, uuid, folder_slug, version_number, game_id, slot_id, tag, created_at, parent_id)
SELECT id, uuid, folder_slug, version_number, game_id, NULL, tag, created_at, parent_id FROM versions;

DROP TABLE versions;
ALTER TABLE versions_new RENAME TO versions;

CREATE INDEX idx_versions_game_id ON versions(game_id);
CREATE INDEX idx_versions_slot_id ON versions(slot_id);
CREATE INDEX idx_versions_version_number ON versions(version_number);
CREATE INDEX idx_versions_created_at ON versions(created_at);
CREATE UNIQUE INDEX idx_versions_uuid ON versions(uuid);
CREATE UNIQUE INDEX idx_versions_folder_slug ON versions(folder_slug);
