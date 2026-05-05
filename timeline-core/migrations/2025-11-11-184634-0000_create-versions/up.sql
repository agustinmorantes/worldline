CREATE TABLE versions (
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT ,
    version_number INTEGER NOT NULL,
    game_id INTEGER NOT NULL,
    tag TEXT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP NOT NULL,

    FOREIGN KEY (game_id) REFERENCES games(id) ON DELETE CASCADE,
    UNIQUE (game_id, version_number)
);

CREATE INDEX idx_versions_game_id ON versions(game_id);
CREATE INDEX idx_versions_version_number ON versions(version_number);
CREATE INDEX idx_versions_created_at ON versions(created_at);


-- Modify games table to add active_version_id column
CREATE TABLE games_new(
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL UNIQUE,
    active_version_id INTEGER
);

INSERT INTO games_new(id, title)
SELECT id, title FROM games;

DROP TABLE games;
ALTER TABLE games_new RENAME TO games;

CREATE UNIQUE INDEX idx_games_title ON games(title);
