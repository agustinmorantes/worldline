DROP INDEX IF EXISTS idx_versions_folder_slug;
DROP INDEX IF EXISTS idx_versions_uuid;
DROP INDEX IF EXISTS idx_tracked_dirs_folder_slug;
DROP INDEX IF EXISTS idx_tracked_dirs_uuid;
DROP INDEX IF EXISTS idx_games_folder_slug;
DROP INDEX IF EXISTS idx_games_title_normalized;
DROP INDEX IF EXISTS idx_games_uuid;

CREATE TABLE versions_new (
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    version_number INTEGER NOT NULL,
    game_id INTEGER NOT NULL,
    tag TEXT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP NOT NULL,
    parent_id INTEGER REFERENCES versions(id),
    FOREIGN KEY (game_id) REFERENCES games(id) ON DELETE CASCADE,
    UNIQUE (game_id, version_number)
);

INSERT INTO versions_new (id, version_number, game_id, tag, created_at, parent_id)
SELECT id, version_number, game_id, tag, created_at, parent_id FROM versions;

DROP TABLE versions;
ALTER TABLE versions_new RENAME TO versions;

CREATE INDEX idx_versions_game_id ON versions(game_id);
CREATE INDEX idx_versions_version_number ON versions(version_number);
CREATE INDEX idx_versions_created_at ON versions(created_at);

CREATE TABLE tracked_dirs_new (
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    path TEXT NOT NULL UNIQUE,
    game_id INTEGER NOT NULL,
    FOREIGN KEY (game_id) REFERENCES games(id) ON DELETE CASCADE
);

INSERT INTO tracked_dirs_new (id, path, game_id)
SELECT id, path, game_id FROM tracked_dirs;

DROP TABLE tracked_dirs;
ALTER TABLE tracked_dirs_new RENAME TO tracked_dirs;

CREATE INDEX idx_tracked_dirs_game_id ON tracked_dirs(game_id);
CREATE INDEX idx_tracked_dirs_path ON tracked_dirs(path);

CREATE TABLE games_new (
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL UNIQUE,
    active_version_id INTEGER,
    icon_path TEXT
);

INSERT INTO games_new (id, title, active_version_id, icon_path)
SELECT id, title, active_version_id, icon_path FROM games;

DROP TABLE games;
ALTER TABLE games_new RENAME TO games;

CREATE UNIQUE INDEX idx_games_title ON games(title);
