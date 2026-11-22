-- Run-now trigger, quotas, GC audit helpers

ALTER TABLE agents
    ADD COLUMN IF NOT EXISTS run_now BOOLEAN NOT NULL DEFAULT FALSE;

ALTER TABLE users
    ADD COLUMN IF NOT EXISTS quota_bytes BIGINT;

ALTER TABLE storage_targets
    ADD COLUMN IF NOT EXISTS used_bytes BIGINT NOT NULL DEFAULT 0;

CREATE TABLE IF NOT EXISTS gc_runs (
    id              BIGSERIAL PRIMARY KEY,
    started_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    finished_at     TIMESTAMPTZ,
    snapshots_deleted INT NOT NULL DEFAULT 0,
    chunks_deleted  INT NOT NULL DEFAULT 0,
    bytes_reclaimed BIGINT NOT NULL DEFAULT 0,
    message         TEXT NOT NULL DEFAULT ''
);
