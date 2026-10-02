CREATE TABLE workspaces (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    root TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL
);

CREATE INDEX idx_workspaces_created
ON workspaces(created_at DESC, id DESC);

CREATE TABLE chat_sessions (
    id TEXT PRIMARY KEY NOT NULL,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    title TEXT,
    path_allow_read TEXT NOT NULL DEFAULT '[]',
    path_allow_write TEXT NOT NULL DEFAULT '[]',
    path_deny_read TEXT NOT NULL DEFAULT '[]',
    path_deny_write TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_used_at TEXT NOT NULL
);

CREATE INDEX idx_chat_sessions_workspace_last_used
ON chat_sessions(workspace_id, last_used_at DESC, id DESC);

CREATE TABLE chat_messages (
    id TEXT PRIMARY KEY NOT NULL,
    chat_session_id TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    body TEXT NOT NULL,
    UNIQUE (chat_session_id, position)
);
