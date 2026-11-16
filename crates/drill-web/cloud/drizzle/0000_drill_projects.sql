CREATE TABLE drill_projects (
    owner TEXT NOT NULL,
    id TEXT NOT NULL,
    title TEXT NOT NULL,
    document TEXT NOT NULL,
    revision INTEGER NOT NULL DEFAULT 1,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (owner, id)
);
CREATE INDEX drill_projects_recent ON drill_projects (owner, updated_at DESC, id);
