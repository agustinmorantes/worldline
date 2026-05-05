CREATE TABLE tracked_dirs(
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    path TEXT NOT NULL UNIQUE,
    game_id INTEGER NOT NULL,
    FOREIGN KEY (game_id) REFERENCES games(id) ON DELETE CASCADE
);

CREATE INDEX idx_tracked_dirs_game_id ON tracked_dirs(game_id);
CREATE INDEX idx_tracked_dirs_path ON tracked_dirs(path);
