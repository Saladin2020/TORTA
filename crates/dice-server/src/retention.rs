use std::collections::HashSet;
use std::time::Duration;

use sqlx::PgPool;

use crate::notify;
use crate::quota::set_store_used;
use crate::state::AppState;

#[derive(Debug, Default)]
pub struct RetentionReport {
    pub snapshots_deleted: i32,
    pub chunks_deleted: i32,
    pub bytes_reclaimed: i64,
    pub stale_failed: i32,
    pub message: String,
}

/// Delete snapshots older than each agent's/policy retention, then GC orphan chunks.
pub async fn run_retention(state: &AppState) -> anyhow::Result<RetentionReport> {
    let mut report = RetentionReport::default();

    // Fail stale running snapshots (crash recovery)
    let stale = sqlx::query(
        r#"UPDATE snapshots SET
             status = 'failed',
             finished_at = NOW(),
             error_message = COALESCE(error_message, 'stale: marked failed by retention (agent/server crash?)')
           WHERE status IN ('running', 'queued')
             AND started_at < NOW() - make_interval(hours => $1)
             AND deleted_at IS NULL"#,
    )
    .bind(state.config.stale_snapshot_hours as i32)
    .execute(&state.db)
    .await?
    .rows_affected();
    report.stale_failed = stale as i32;

    // Hard-delete soft-deleted snapshots past grace (7 days) OR aged by retention
    let deleted = sqlx::query(
        r#"
        WITH aged AS (
          SELECT s.id
          FROM snapshots s
          LEFT JOIN agents a ON a.id = s.agent_id
          LEFT JOIN policies p ON p.id = a.policy_id
          WHERE (
              (s.deleted_at IS NOT NULL AND s.deleted_at < NOW() - INTERVAL '7 days')
              OR (
                s.deleted_at IS NULL
                AND s.status IN ('success', 'failed', 'cancelled')
                AND s.started_at < NOW() - make_interval(days => COALESCE(p.retention_days, 30))
              )
            )
        )
        DELETE FROM snapshots s
        USING aged
        WHERE s.id = aged.id
        "#,
    )
    .execute(&state.db)
    .await?
    .rows_affected();

    report.snapshots_deleted = deleted as i32;

    let live = collect_live_hashes(&state.db).await?;
    let gc = tokio::task::spawn_blocking({
        let path = state.config.store_path.clone();
        let live = live.clone();
        move || dice_core::gc_unreferenced(&path, &live)
    })
    .await??;

    report.chunks_deleted = gc.chunks_deleted as i32;
    report.bytes_reclaimed = gc.bytes_reclaimed as i64;
    report.message = format!(
        "stale_failed={}, deleted {} snapshots, {} orphan chunks, reclaimed {} bytes ({} live chunks)",
        report.stale_failed,
        report.snapshots_deleted,
        report.chunks_deleted,
        report.bytes_reclaimed,
        gc.live_chunks
    );

    let used = dice_core::measure_store_bytes(&state.config.store_path)?;
    set_store_used(
        &state.db,
        &state.config.store_path.display().to_string(),
        used as i64,
    )
    .await?;

    sqlx::query(
        r#"INSERT INTO gc_runs (finished_at, snapshots_deleted, chunks_deleted, bytes_reclaimed, message)
           VALUES (NOW(), $1, $2, $3, $4)"#,
    )
    .bind(report.snapshots_deleted)
    .bind(report.chunks_deleted)
    .bind(report.bytes_reclaimed)
    .bind(&report.message)
    .execute(&state.db)
    .await?;

    if report.stale_failed > 0 {
        notify::notify_webhook(
            &state.config.notify_webhook_url,
            "snapshot.stale_failed",
            serde_json::json!({ "count": report.stale_failed }),
        )
        .await;
    }

    tracing::info!(%report.message, "retention GC finished");
    Ok(report)
}

async fn collect_live_hashes(db: &PgPool) -> anyhow::Result<HashSet<String>> {
    let rows: Vec<(serde_json::Value,)> =
        sqlx::query_as("SELECT manifest_json FROM snapshot_files")
            .fetch_all(db)
            .await?;

    let mut live = HashSet::new();
    for (manifest,) in rows {
        if let Ok(hashes) = dice_core::extract_hashes_from_manifest_json(&manifest) {
            live.extend(hashes);
        }
    }
    Ok(live)
}

pub fn spawn_retention_loop(state: AppState) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(30)).await;
        loop {
            if let Err(e) = run_retention(&state).await {
                tracing::error!(error = %e, "retention GC failed");
            }
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    });
}
