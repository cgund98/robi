CREATE TABLE tool_originals (
  id TEXT PRIMARY KEY,
  chat_session_id TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
  tool_call_id TEXT NOT NULL,
  sha256 TEXT NOT NULL,
  body TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE INDEX tool_originals_session ON tool_originals (chat_session_id, sha256);
