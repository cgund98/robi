-- S9: plans are workspace files under `.robi/plans`, so a session no longer
-- points at one under `~/.robi/plans/<session_id>`.
ALTER TABLE chat_sessions DROP COLUMN plan_path;
