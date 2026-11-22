# Disaster recovery runbook

## What must stay together

1. **Postgres dump** (ownership, manifests, policies, users) — `deploy/backup-metadata.sh`
2. **Chunk store directory** (`store_path`) — filesystem/object backup
3. **`store_key_hex`** — without this key encrypted chunks are unreadable

Losing any one of these without the others makes restores incomplete or impossible.

## Restore steps

1. Restore Postgres: `gunzip -c dice-meta-….sql.gz | psql "$DATABASE_URL"`
2. Restore `store_path` tree to the path in `dice-server.toml`
3. Put the same `store_key_hex` / `JWT_SECRET` into config
4. Start `dice-server`, confirm `GET /api/health` → `ok: true`
5. Spot-check: portal → select file → **Verify** (reassemble + BLAKE3)
6. Optional: download snapshot ZIP

## Soft-delete / trash

Users can trash snapshots in the portal. Hard delete happens after ~7 days via retention GC (or sooner if retention days expire).
