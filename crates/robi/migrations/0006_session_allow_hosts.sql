ALTER TABLE chat_sessions
ADD COLUMN allow_hosts TEXT NOT NULL DEFAULT '[]';
