CREATE TABLE users (
  id INT NOT NULL,
  legacy_name TEXT,
  display_name TEXT
);

ALTER TABLE users ADD COLUMN nickname TEXT;
UPDATE users SET nickname = display_name;
ALTER TABLE users RENAME COLUMN nickname TO public_name;
