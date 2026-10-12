-- The content hash of the icon at icon_path, so the GUI can tell a replaced
-- icon from the one it already shows at the same path. Projected from the
-- repository files like icon_path; reindex fills it on open.
ALTER TABLE games ADD COLUMN icon_sha256 TEXT;
