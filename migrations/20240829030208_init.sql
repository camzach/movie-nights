-- Add migration script here
CREATE TABLE movies (imdb_id varchar(20) PRIMARY KEY, vetos int NOT NULL DEFAULT 0, watched date, proposed_on date NOT NULL, proposed_by varchar(20) NOT NULL);
