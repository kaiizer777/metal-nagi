-- Up Migration: Seed OpenCode Go provider and fallback models

ALTER TABLE providers ADD COLUMN IF NOT EXISTS base_url text;

INSERT INTO providers (name, priority, api_key_env_var, base_url, is_enabled)
VALUES (
    'opencode_go',
    COALESCE(
        (SELECT priority FROM providers WHERE name = 'opencode_go'),
        (SELECT COALESCE(MAX(priority), 0) + 1 FROM providers)
    ),
    'OPENCODE_API_KEY',
    'https://opencode.ai/inference/go/openai/v1',
    true
)
ON CONFLICT (name) DO UPDATE SET
    api_key_env_var = EXCLUDED.api_key_env_var,
    base_url = EXCLUDED.base_url,
    is_enabled = true;

INSERT INTO provider_models (provider_id, model_name, priority, is_enabled)
SELECT
    id,
    'beam-free',
    COALESCE(
        (SELECT priority FROM provider_models WHERE provider_id = providers.id AND model_name = 'beam-free'),
        (SELECT COALESCE(MAX(priority), 0) + 1 FROM provider_models WHERE provider_id = providers.id)
    ),
    true
FROM providers WHERE name = 'opencode_go'
ON CONFLICT (provider_id, model_name) DO UPDATE SET
    is_enabled = true;

INSERT INTO provider_models (provider_id, model_name, priority, is_enabled)
SELECT
    id,
    'iceman-contributor-free',
    COALESCE(
        (SELECT priority FROM provider_models WHERE provider_id = providers.id AND model_name = 'iceman-contributor-free'),
        (SELECT COALESCE(MAX(priority), 0) + 1 FROM provider_models WHERE provider_id = providers.id)
    ),
    true
FROM providers WHERE name = 'opencode_go'
ON CONFLICT (provider_id, model_name) DO UPDATE SET
    is_enabled = true;

INSERT INTO provider_models (provider_id, model_name, priority, is_enabled)
SELECT
    id,
    'muse-spark-1.3-contributor',
    COALESCE(
        (SELECT priority FROM provider_models WHERE provider_id = providers.id AND model_name = 'muse-spark-1.3-contributor'),
        (SELECT COALESCE(MAX(priority), 0) + 1 FROM provider_models WHERE provider_id = providers.id)
    ),
    true
FROM providers WHERE name = 'opencode_go'
ON CONFLICT (provider_id, model_name) DO UPDATE SET
    is_enabled = true;

INSERT INTO provider_models (provider_id, model_name, priority, is_enabled)
SELECT
    id,
    'mimo-v2.6-flash',
    COALESCE(
        (SELECT priority FROM provider_models WHERE provider_id = providers.id AND model_name = 'mimo-v2.6-flash'),
        (SELECT COALESCE(MAX(priority), 0) + 1 FROM provider_models WHERE provider_id = providers.id)
    ),
    true
FROM providers WHERE name = 'opencode_go'
ON CONFLICT (provider_id, model_name) DO UPDATE SET
    is_enabled = true;

