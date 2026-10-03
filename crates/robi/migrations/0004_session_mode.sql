ALTER TABLE chat_sessions
ADD COLUMN mode TEXT NOT NULL DEFAULT 'agent';

UPDATE chat_sessions
SET model_config = json_object(
    'agent',
    json(model_config),
    'ask',
    json('{}'),
    'plan',
    json('{}')
)
WHERE json_type(model_config, '$.agent') IS NULL
  AND json_type(model_config, '$.ask') IS NULL
  AND json_type(model_config, '$.plan') IS NULL;
