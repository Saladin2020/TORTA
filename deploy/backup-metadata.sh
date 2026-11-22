#!/usr/bin/env bash
# Disaster recovery: dump Postgres metadata + record store path.
# Schedule daily via cron. Keep dumps off-box with the chunk store backups.
set -euo pipefail

CONFIG="${DICE_SERVER_TOML:-/etc/dice/dice-server.toml}"
OUT_DIR="${DICE_BACKUP_DIR:-/var/backups/dice}"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
mkdir -p "$OUT_DIR"

if [[ -f "$CONFIG" ]]; then
  # shellcheck disable=SC1091
  DATABASE_URL="$(grep -E '^database_url' "$CONFIG" | head -1 | cut -d'"' -f2)"
  STORE_PATH="$(grep -E '^store_path' "$CONFIG" | head -1 | cut -d'"' -f2)"
else
  DATABASE_URL="${DATABASE_URL:?set DATABASE_URL or provide config}"
  STORE_PATH="${DICE_STORE_PATH:-/var/lib/dice/store}"
fi

DUMP="$OUT_DIR/dice-meta-$STAMP.sql.gz"
echo "dumping $DATABASE_URL -> $DUMP"
pg_dump "$DATABASE_URL" | gzip -9 > "$DUMP"

META="$OUT_DIR/dice-meta-$STAMP.json"
cat > "$META" <<EOF
{
  "ts": "$STAMP",
  "store_path": "$STORE_PATH",
  "dump": "$(basename "$DUMP")"
}
EOF

# Keep 14 days
find "$OUT_DIR" -name 'dice-meta-*.sql.gz' -mtime +14 -delete
find "$OUT_DIR" -name 'dice-meta-*.json' -mtime +14 -delete

echo "ok: $DUMP"
echo "Restore: gunzip -c $DUMP | psql \$DATABASE_URL"
echo "Store: restore $STORE_PATH from filesystem/object backup with matching store_key_hex"
