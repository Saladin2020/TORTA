-- TORTA Dice org backup schema
CREATE EXTENSION IF NOT EXISTS "pgcrypto";

CREATE TYPE auth_provider AS ENUM ('local', 'ldap');
CREATE TYPE user_role AS ENUM ('admin', 'user');
CREATE TYPE agent_status AS ENUM ('online', 'offline', 'disabled');
CREATE TYPE job_status AS ENUM ('queued', 'running', 'success', 'failed', 'cancelled');
CREATE TYPE snapshot_source AS ENUM ('agent', 'web_upload');

CREATE TABLE users (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    username        TEXT NOT NULL UNIQUE,
    display_name    TEXT NOT NULL DEFAULT '',
    email           TEXT,
    role            user_role NOT NULL DEFAULT 'user',
    auth_provider   auth_provider NOT NULL DEFAULT 'local',
    password_hash   TEXT,
    ldap_dn         TEXT,
    enabled         BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE storage_targets (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name            TEXT NOT NULL UNIQUE,
    path            TEXT NOT NULL,
    is_default      BOOLEAN NOT NULL DEFAULT FALSE,
    quota_bytes     BIGINT,
    notes           TEXT NOT NULL DEFAULT '',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE policies (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name            TEXT NOT NULL UNIQUE,
    enabled         BOOLEAN NOT NULL DEFAULT TRUE,
    -- cron-like: "0 2 * * *" = 02:00 daily (agent interprets hour/minute for v1)
    schedule_cron   TEXT NOT NULL DEFAULT '0 2 * * *',
    paths_json      JSONB NOT NULL DEFAULT '[]'::jsonb,
    excludes_json   JSONB NOT NULL DEFAULT '[]'::jsonb,
    retention_days  INT NOT NULL DEFAULT 30,
    bandwidth_bps   BIGINT,
    storage_target_id UUID REFERENCES storage_targets(id) ON DELETE SET NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE agents (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    policy_id       UUID REFERENCES policies(id) ON DELETE SET NULL,
    name            TEXT NOT NULL,
    hostname        TEXT NOT NULL DEFAULT '',
    os              TEXT NOT NULL DEFAULT '',
    agent_token     TEXT NOT NULL UNIQUE,
    enabled         BOOLEAN NOT NULL DEFAULT TRUE,
    local_enabled   BOOLEAN NOT NULL DEFAULT TRUE,
    status          agent_status NOT NULL DEFAULT 'offline',
    last_seen_at    TIMESTAMPTZ,
    last_backup_at  TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (user_id, name)
);

CREATE TABLE snapshots (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    agent_id        UUID REFERENCES agents(id) ON DELETE SET NULL,
    storage_target_id UUID REFERENCES storage_targets(id) ON DELETE SET NULL,
    source          snapshot_source NOT NULL,
    label           TEXT NOT NULL DEFAULT '',
    started_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    finished_at     TIMESTAMPTZ,
    status          job_status NOT NULL DEFAULT 'running',
    file_count      INT NOT NULL DEFAULT 0,
    raw_bytes       BIGINT NOT NULL DEFAULT 0,
    stored_bytes    BIGINT NOT NULL DEFAULT 0,
    error_message   TEXT
);

CREATE TABLE snapshot_files (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    snapshot_id     UUID NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
    relative_path   TEXT NOT NULL,
    size_bytes      BIGINT NOT NULL,
    content_hash    TEXT NOT NULL,
    manifest_json   JSONB NOT NULL,
    UNIQUE (snapshot_id, relative_path)
);

CREATE INDEX idx_snapshot_files_snapshot ON snapshot_files(snapshot_id);
CREATE INDEX idx_snapshots_user ON snapshots(user_id, started_at DESC);
CREATE INDEX idx_agents_token ON agents(agent_token);
CREATE INDEX idx_agents_user ON agents(user_id);

CREATE TABLE jobs (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    agent_id        UUID REFERENCES agents(id) ON DELETE CASCADE,
    snapshot_id     UUID REFERENCES snapshots(id) ON DELETE SET NULL,
    kind            TEXT NOT NULL DEFAULT 'backup',
    status          job_status NOT NULL DEFAULT 'queued',
    message         TEXT NOT NULL DEFAULT '',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE audit_log (
    id              BIGSERIAL PRIMARY KEY,
    actor_user_id   UUID REFERENCES users(id) ON DELETE SET NULL,
    action          TEXT NOT NULL,
    detail          JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Bootstrap admin: password "admin123" (change immediately)
-- hash generated at first boot if no users exist
