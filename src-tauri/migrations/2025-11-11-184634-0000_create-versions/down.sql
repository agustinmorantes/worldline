-- Restore games table to original structure (remove active_version_id)
CREATE TABLE games_old(
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL UNIQUE
);

INSERT INTO games_old(id, title)
SELECT id, title FROM games;

DROP TABLE games;
ALTER TABLE games_old RENAME TO games;

CREATE UNIQUE INDEX idx_games_title ON games(title);

-- Drop versions table
DROP TABLE IF EXISTS versions;
