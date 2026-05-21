CREATE TABLE versions_new (
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    uuid TEXT,
    folder_slug TEXT,
    version_number INTEGER NOT NULL,
    game_id INTEGER NOT NULL,
    tag TEXT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP NOT NULL,
    parent_id INTEGER REFERENCES versions(id),
    FOREIGN KEY (game_id) REFERENCES games(id) ON DELETE CASCADE,
    UNIQUE (game_id, version_number)
);

INSERT INTO versions_new (id, uuid, folder_slug, version_number, game_id, tag, created_at, parent_id)
SELECT id, uuid, folder_slug, version_number, game_id, tag, created_at, parent_id FROM versions;

DROP TABLE versions;
ALTER TABLE versions_new RENAME TO versions;

CREATE INDEX idx_versions_game_id ON versions(game_id);
CREATE INDEX idx_versions_version_number ON versions(version_number);
CREATE INDEX idx_versions_created_at ON versions(created_at);
CREATE UNIQUE INDEX idx_versions_uuid ON versions(uuid);
CREATE UNIQUE INDEX idx_versions_folder_slug ON versions(folder_slug);

CREATE TABLE games_new (
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    uuid TEXT,
    title TEXT NOT NULL,
    title_normalized TEXT,
    folder_slug TEXT,
    active_version_id INTEGER,
    icon_path TEXT
);

INSERT INTO games_new (id, uuid, title, title_normalized, folder_slug, active_version_id, icon_path)
SELECT id, uuid, title, title_normalized, folder_slug, active_version_id, icon_path FROM games;

DROP TABLE games;
ALTER TABLE games_new RENAME TO games;

CREATE UNIQUE INDEX idx_games_title ON games(title);
CREATE UNIQUE INDEX idx_games_uuid ON games(uuid);
CREATE UNIQUE INDEX idx_games_title_normalized ON games(title_normalized);
CREATE UNIQUE INDEX idx_games_folder_slug ON games(folder_slug);

DROP TABLE slots;
