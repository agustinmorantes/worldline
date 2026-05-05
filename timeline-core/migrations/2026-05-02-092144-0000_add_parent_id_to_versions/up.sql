ALTER TABLE versions ADD COLUMN parent_id INTEGER REFERENCES versions(id);
