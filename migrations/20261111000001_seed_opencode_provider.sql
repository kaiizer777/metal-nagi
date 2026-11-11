-- Up Migration: Seed OpenCode Go provider and fallback models

ALTER TABLE providers ADD COLUMN IF NOT EXISTS base_url text;

INSERT INTO providers (name, priority, api_key_env_var, base_url, is_enabled)
VALUES ('opencode_go', 1, 'OPENCODE_API_KEY', 'https://opencode.ai/inference/go/openai/v1', true)
ON CONFLICT (name) DO UPDATE SET
    priority = EXCLUDED.priority,
    api_key_env_var = EXCLUDED.api_key_env_var,
    base_url = EXCLUDED.base_url,
    is_enabled = EXCLUDED.is_enabled;

INSERT INTO provider_models (provider_id, model_name, priority, is_enabled)
SELECT id, 'beam-free', 1, true FROM providers WHERE name = 'opencode_go'
ON CONFLICT (provider_id, model_name) DO UPDATE SET
    priority = EXCLUDED.priority,
    is_enabled = EXCLUDED.is_enabled;

INSERT INTO provider_models (provider_id, model_name, priority, is_enabled)
SELECT id, 'iceman-contributor-free', 2, true FROM providers WHERE name = 'opencode_go'
ON CONFLICT (provider_id, model_name) DO UPDATE SET
    priority = EXCLUDED.priority,
    is_enabled = EXCLUDED.is_enabled;

INSERT INTO provider_models (provider_id, model_name, priority, is_enabled)
SELECT id, 'muse-spark-1.3-contributor', 3, true FROM providers WHERE name = 'opencode_go'
ON CONFLICT (provider_id, model_name) DO UPDATE SET
    priority = EXCLUDED.priority,
    is_enabled = EXCLUDED.is_enabled;

INSERT INTO provider_models (provider_id, model_name, priority, is_enabled)
SELECT id, 'mimo-v2.6-flash', 4, true FROM providers WHERE name = 'opencode_go'
ON CONFLICT (provider_id, model_name) DO UPDATE SET
    priority = EXCLUDED.priority,
    is_enabled = EXCLUDED.is_enabled;
