ALTER TABLE workspaces ADD COLUMN mcp_project_sha256 TEXT;

ALTER TABLE chat_sessions ADD COLUMN mcp_allows TEXT NOT NULL DEFAULT '[]';
