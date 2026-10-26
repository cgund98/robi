-- Repairing `turn_display` from the transcript read every transcript in the
-- database at every start. `turn_settled` scopes that walk: it is set by the
-- write that stores a display the transcript agrees with, and left clear by a
-- turn in flight. Startup then reads only the sessions a previous process was
-- mid-turn in.
ALTER TABLE chat_sessions ADD COLUMN turn_settled INTEGER NOT NULL DEFAULT 0;

-- The same rule the code uses: only a `pending` display is written while a turn
-- is still running. Everything else was stored by a process that had finished
-- with the row, so an existing database pays the old full walk no more than once.
UPDATE chat_sessions SET turn_settled = 1 WHERE turn_display != 'pending';
