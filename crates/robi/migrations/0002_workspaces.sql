CREATE TABLE workspaces (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    root TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL
);

CREATE INDEX idx_workspaces_created
ON workspaces(created_at DESC, id DESC);

DELETE FROM chat_messages;
DELETE FROM chat_sessions;

CREATE TABLE chat_sessions_next (
    id TEXT PRIMARY KEY NOT NULL,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    title TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_used_at TEXT NOT NULL
);

DROP TABLE chat_sessions;
ALTER TABLE chat_sessions_next RENAME TO chat_sessions;

CREATE INDEX idx_chat_sessions_workspace_last_used
ON chat_sessions(workspace_id, last_used_at DESC, id DESC);
