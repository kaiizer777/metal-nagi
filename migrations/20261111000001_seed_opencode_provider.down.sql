-- Down Migration: Remove OpenCode Go provider and models
DELETE FROM provider_models 
WHERE provider_id IN (SELECT id FROM providers WHERE name = 'opencode_go')
  AND model_name IN ('beam-free', 'iceman-contributor-free', 'muse-spark-1.3-contributor', 'mimo-v2.6-flash');
DELETE FROM providers WHERE name = 'opencode_go';

