CREATE TABLE session_file_baselines (
    chat_session_id TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    baseline TEXT NOT NULL,
    created INTEGER NOT NULL,
    PRIMARY KEY (chat_session_id, path)
);
