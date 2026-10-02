ALTER TABLE chat_sessions
ADD COLUMN model_config TEXT NOT NULL DEFAULT '{}';
