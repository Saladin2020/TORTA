use sqlx::PgPool;
use uuid::Uuid;

use crate::state::AppState;

#[derive(Debug)]
pub struct QuotaError {
    pub message: String,
}

pub async fn assert_can_store(
    state: &AppState,
    user_id: Uuid,
    additional_bytes: i64,
) -> Result<(), QuotaError> {
    // Storage target quota (default store)
    let store: Option<(Option<i64>, i64)> = sqlx::query_as(
        r#"SELECT quota_bytes, used_bytes FROM storage_targets
           WHERE is_default = TRUE OR path = $1
           ORDER BY is_default DESC
           LIMIT 1"#,
    )
    .bind(state.config.store_path.display().to_string())
    .fetch_optional(&state.db)
    .await
    .map_err(|e| QuotaError {
        message: e.to_string(),
    })?;

    if let Some((Some(quota), used)) = store {
        if used + additional_bytes > quota {
            return Err(QuotaError {
                message: format!(
                    "storage quota exceeded: used {used} + {additional_bytes} > {quota}"
                ),
            });
        }
    }

    // Per-user quota (sum of successful snapshot stored_bytes)
    let user_q: (Option<i64>,) = sqlx::query_as("SELECT quota_bytes FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(&state.db)
        .await
        .map_err(|e| QuotaError {
            message: e.to_string(),
        })?;

    if let Some(quota) = user_q.0 {
        let used: (i64,) = sqlx::query_as(
            r#"SELECT COALESCE(SUM(stored_bytes), 0)
               FROM snapshots
               WHERE user_id = $1 AND status = 'success'"#,
        )
        .bind(user_id)
        .fetch_one(&state.db)
        .await
        .map_err(|e| QuotaError {
            message: e.to_string(),
        })?;
        if used.0 + additional_bytes > quota {
            return Err(QuotaError {
                message: format!(
                    "user quota exceeded: used {} + {additional_bytes} > {quota}",
                    used.0
                ),
            });
        }
    }

    Ok(())
}

pub async fn bump_store_used(db: &PgPool, store_path: &str, delta: i64) -> anyhow::Result<()> {
    sqlx::query(
        r#"UPDATE storage_targets
           SET used_bytes = GREATEST(0, used_bytes + $2)
           WHERE path = $1 OR is_default = TRUE"#,
    )
    .bind(store_path)
    .bind(delta)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn set_store_used(db: &PgPool, store_path: &str, used: i64) -> anyhow::Result<()> {
    sqlx::query(
        r#"UPDATE storage_targets SET used_bytes = $2
           WHERE path = $1 OR is_default = TRUE"#,
    )
    .bind(store_path)
    .bind(used)
    .execute(db)
    .await?;
    Ok(())
}
