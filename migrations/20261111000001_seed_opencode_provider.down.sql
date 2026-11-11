-- Down Migration: Remove OpenCode Go provider and models
DELETE FROM provider_models WHERE provider_id IN (SELECT id FROM providers WHERE name = 'opencode_go');
DELETE FROM providers WHERE name = 'opencode_go';
