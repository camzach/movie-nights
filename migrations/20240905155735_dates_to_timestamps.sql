-- Add migration script here
ALTER TABLE movies ADD watched_at timestamp;
ALTER TABLE movies ADD proposed_at timestamp;
UPDATE movies SET watched_at = watched;
UPDATE movies SET proposed_at = proposed_on;
ALTER TABLE movies ALTER COLUMN proposed_at SET NOT NULL;
ALTER TABLE movies DROP COLUMN watched;
ALTER TABLE movies DROP COLUMN proposed_on;
