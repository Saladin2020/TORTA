# Security & production checklist

## Before production

1. Set `production = true` in `dice-server.toml` (or `DICE_PRODUCTION=true`).
2. Strong `jwt_secret` (≥24 chars, not a placeholder) — `openssl rand -base64 48`.
3. `store_key_hex` — `openssl rand -hex 32` (at-rest ChaCha20-Poly1305).
4. `cors_origins` — exact HTTPS origins of the portal.
5. Terminate TLS with nginx/caddy (`deploy/nginx.conf.example`).
6. Bootstrap admin: set `DICE_BOOTSTRAP_ADMIN_PASSWORD` on first boot, or change `admin123` immediately (forced when default).
7. Prefer `ldaps://` for LDAP; usernames are filter-escaped.
8. Schedule `deploy/backup-metadata.sh` + filesystem/object backup of the chunk store **with the same key**.

## Auth model

| Item | Behavior |
|------|----------|
| Passwords | Argon2id |
| JWT | Configurable TTL (default 8h) |
| Login abuse | In-memory rate limit + DB lockout after 10 failures |
| Agent tokens | BLAKE3 hashed at rest (plaintext shown once on create) |
| Roles | `admin`, `auditor` (read ops + audit), `user` |

## Integrity

- `POST /api/portal/snapshots/{id}/files/{file_id}/verify` reassembles chunks and checks BLAKE3.
- Stale `running` snapshots auto-fail after `stale_snapshot_hours`.
- Soft-delete (`trash`) then hard-delete via retention (≥7 days in trash).

## Notifications

Set `notify_webhook_url` / `DICE_NOTIFY_WEBHOOK` to receive JSON on backup failure and stale snapshots.

## Health

`GET /api/health` checks DB connectivity and store writability.
