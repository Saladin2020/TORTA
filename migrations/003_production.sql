-- Production hardening: password policy, soft-delete, login lockout, roles

ALTER TABLE users
    ADD COLUMN IF NOT EXISTS must_change_password BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS password_changed_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS failed_login_count INT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS locked_until TIMESTAMPTZ;

-- Soft-delete snapshots (trash) before hard GC
ALTER TABLE snapshots
    ADD COLUMN IF NOT EXISTS deleted_at TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS idx_snapshots_deleted ON snapshots(deleted_at)
    WHERE deleted_at IS NOT NULL;

-- Auditor role (read-only ops visibility). PG16+ allows this in a transaction.
ALTER TYPE user_role ADD VALUE IF NOT EXISTS 'auditor';

CREATE TABLE IF NOT EXISTS login_attempts (
    id              BIGSERIAL PRIMARY KEY,
    username        TEXT NOT NULL,
    ip              TEXT NOT NULL DEFAULT '',
    success         BOOLEAN NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_login_attempts_user_time
    ON login_attempts(username, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_audit_log_created
    ON audit_log(created_at DESC);

-- Mark bootstrap admin if still using default (set at runtime); no-op placeholder
COMMENT ON COLUMN users.must_change_password IS 'Force password change after login (local accounts)';
