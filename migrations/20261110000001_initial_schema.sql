-- Up Migration: Initial Schema v1 for metal-nagi
-- Extensions
CREATE EXTENSION IF NOT EXISTS "pgcrypto";

-- 1. jobs table
CREATE TABLE IF NOT EXISTS jobs (
    job_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    repo TEXT NOT NULL,
    issue_or_pr_number INT NOT NULL,
    pipeline_type VARCHAR(1) NOT NULL CHECK (pipeline_type IN ('A', 'B')),
    status VARCHAR(32) NOT NULL,
    current_phase INT NOT NULL DEFAULT 1,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Active-Job concurrency lock index: guarantees at most one active job per (repo, issue_or_pr_number)
CREATE UNIQUE INDEX IF NOT EXISTS idx_jobs_active_lock 
    ON jobs (repo, issue_or_pr_number) 
    WHERE status = 'active';

CREATE INDEX IF NOT EXISTS idx_jobs_status_repo 
    ON jobs (status, repo);

-- 2. job_phases table
CREATE TABLE IF NOT EXISTS job_phases (
    id BIGSERIAL PRIMARY KEY,
    job_id UUID NOT NULL REFERENCES jobs(job_id) ON DELETE CASCADE,
    phase INT NOT NULL,
    status VARCHAR(32) NOT NULL,
    input_payload JSONB,
    output_payload JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_job_phases_job_id_phase 
    ON job_phases (job_id, phase);

-- 3. coder_outputs table
CREATE TABLE IF NOT EXISTS coder_outputs (
    id BIGSERIAL PRIMARY KEY,
    job_id UUID NOT NULL REFERENCES jobs(job_id) ON DELETE CASCADE,
    coder_index INT NOT NULL CHECK (coder_index BETWEEN 1 AND 5),
    task_slice TEXT,
    diff_content TEXT,
    status VARCHAR(32) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at TIMESTAMPTZ,
    UNIQUE (job_id, coder_index)
);

CREATE INDEX IF NOT EXISTS idx_coder_outputs_job_id 
    ON coder_outputs (job_id);

-- 4. providers table
CREATE TABLE IF NOT EXISTS providers (
    id BIGSERIAL PRIMARY KEY,
    name TEXT UNIQUE NOT NULL,
    priority INT UNIQUE NOT NULL,
    api_key_env_var TEXT NOT NULL,
    is_enabled BOOLEAN NOT NULL DEFAULT true
);

-- 5. provider_models table
CREATE TABLE IF NOT EXISTS provider_models (
    id BIGSERIAL PRIMARY KEY,
    provider_id BIGINT NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
    model_name TEXT NOT NULL,
    priority INT NOT NULL,
    is_enabled BOOLEAN NOT NULL DEFAULT true,
    UNIQUE (provider_id, model_name),
    UNIQUE (provider_id, priority)
);

CREATE INDEX IF NOT EXISTS idx_provider_models_provider_id 
    ON provider_models (provider_id);

-- 6. repo_configs table
CREATE TABLE IF NOT EXISTS repo_configs (
    repo TEXT PRIMARY KEY,
    owner_only BOOLEAN NOT NULL DEFAULT true,
    allowed_senders JSONB NOT NULL DEFAULT '[]'::jsonb,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

