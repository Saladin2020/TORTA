use axum::{
    body::Bytes,
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::AgentAuth;
use crate::state::AppState;

#[derive(Serialize)]
pub struct AgentPolicyResponse {
    pub agent_id: Uuid,
    pub enabled: bool,
    pub local_enabled: bool,
    pub run_now: bool,
    pub schedule_cron: String,
    pub paths: Vec<String>,
    pub excludes: Vec<String>,
    pub retention_days: i32,
    pub bandwidth_bps: Option<i64>,
    pub server_time: DateTime<Utc>,
}

pub async fn get_policy(
    agent: AgentAuth,
    State(state): State<AppState>,
) -> Result<Json<AgentPolicyResponse>, (axum::http::StatusCode, String)> {
    // Atomically claim run_now so only one poll consumes the trigger.
    let claimed: Option<(Uuid,)> = sqlx::query_as(
        r#"UPDATE agents SET run_now = FALSE
           WHERE id = $1 AND run_now = TRUE
           RETURNING id"#,
    )
    .bind(agent.agent_id)
    .fetch_optional(&state.db)
    .await
    .map_err(internal)?;
    let run_now = claimed.is_some();

    let row = sqlx::query_as::<_, (bool, bool, Option<Uuid>)>(
        "SELECT enabled, local_enabled, policy_id FROM agents WHERE id = $1",
    )
    .bind(agent.agent_id)
    .fetch_one(&state.db)
    .await
    .map_err(internal)?;

    let (enabled, local_enabled, policy_id) = row;
    let mut schedule_cron = "0 2 * * *".to_string();
    let mut paths = Vec::new();
    let mut excludes = Vec::new();
    let mut retention_days = 30i32;
    let mut bandwidth_bps = None;
    let mut policy_enabled = true;

    if let Some(pid) = policy_id {
        if let Ok(p) = sqlx::query_as::<
            _,
            (
                bool,
                String,
                serde_json::Value,
                serde_json::Value,
                i32,
                Option<i64>,
            ),
        >(
            r#"SELECT enabled, schedule_cron, paths_json, excludes_json, retention_days, bandwidth_bps
               FROM policies WHERE id = $1"#,
        )
        .bind(pid)
        .fetch_one(&state.db)
        .await
        {
            policy_enabled = p.0;
            schedule_cron = p.1;
            paths = serde_json::from_value(p.2).unwrap_or_default();
            excludes = serde_json::from_value(p.3).unwrap_or_default();
            retention_days = p.4;
            bandwidth_bps = p.5;
        }
    }

    Ok(Json(AgentPolicyResponse {
        agent_id: agent.agent_id,
        enabled: enabled && policy_enabled,
        local_enabled,
        run_now,
        schedule_cron,
        paths,
        excludes,
        retention_days,
        bandwidth_bps,
        server_time: Utc::now(),
    }))
}

#[derive(Deserialize)]
pub struct HeartbeatRequest {
    pub hostname: Option<String>,
    pub os: Option<String>,
    pub local_enabled: Option<bool>,
}

pub async fn heartbeat(
    agent: AgentAuth,
    State(state): State<AppState>,
    Json(body): Json<HeartbeatRequest>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, String)> {
    sqlx::query(
        r#"UPDATE agents SET
             last_seen_at = NOW(),
             status = 'online',
             hostname = COALESCE($2, hostname),
             os = COALESCE($3, os),
             local_enabled = COALESCE($4, local_enabled)
           WHERE id = $1"#,
    )
    .bind(agent.agent_id)
    .bind(body.hostname)
    .bind(body.os)
    .bind(body.local_enabled)
    .execute(&state.db)
    .await
    .map_err(internal)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct MissingRequest {
    pub hashes: Vec<String>,
}

#[derive(Serialize)]
pub struct MissingResponse {
    pub missing: Vec<String>,
}

pub async fn missing_chunks(
    _agent: AgentAuth,
    State(state): State<AppState>,
    Json(body): Json<MissingRequest>,
) -> Result<Json<MissingResponse>, (axum::http::StatusCode, String)> {
    let missing = dice_core::list_missing(&state.config.store_path, &body.hashes);
    Ok(Json(MissingResponse { missing }))
}

pub async fn upload_chunk(
    agent: AgentAuth,
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, String)> {
    let hash = headers
        .get("X-Chunk-Hash")
        .and_then(|v| v.to_str().ok())
        .ok_or((
            axum::http::StatusCode::BAD_REQUEST,
            "missing X-Chunk-Hash".into(),
        ))?;

    crate::quota::assert_can_store(&state, agent.user_id, body.len() as i64)
        .await
        .map_err(|e| (axum::http::StatusCode::INSUFFICIENT_STORAGE, e.message))?;

    let created = dice_core::lock::with_store_file_lock(&state.config.store_path, || {
        dice_core::write_chunk(&state.config.store_path, hash, &body)
    })
    .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if created {
        let _ = dice_core::lock::with_store_file_lock(&state.config.store_path, || {
            dice_core::store::bump_refs(&state.config.store_path, &[hash.to_string()])
        });
        let _ = crate::quota::bump_store_used(
            &state.db,
            &state.config.store_path.display().to_string(),
            body.len() as i64,
        )
        .await;
    }

    Ok(Json(serde_json::json!({ "created": created, "hash": hash })))
}

#[derive(Deserialize)]
pub struct BeginSnapshot {
    pub label: Option<String>,
}

#[derive(Serialize)]
pub struct BeginSnapshotResponse {
    pub snapshot_id: Uuid,
}

pub async fn begin_snapshot(
    agent: AgentAuth,
    State(state): State<AppState>,
    Json(body): Json<BeginSnapshot>,
) -> Result<Json<BeginSnapshotResponse>, (axum::http::StatusCode, String)> {
    let id: (Uuid,) = sqlx::query_as(
        r#"INSERT INTO snapshots (user_id, agent_id, source, label, status)
           VALUES ($1, $2, 'agent', $3, 'running')
           RETURNING id"#,
    )
    .bind(agent.user_id)
    .bind(agent.agent_id)
    .bind(body.label.unwrap_or_default())
    .fetch_one(&state.db)
    .await
    .map_err(internal)?;
    Ok(Json(BeginSnapshotResponse { snapshot_id: id.0 }))
}

#[derive(Deserialize)]
pub struct CommitFile {
    pub relative_path: String,
    pub size_bytes: u64,
    pub content_hash: String,
    pub manifest: serde_json::Value,
}

pub async fn commit_file(
    agent: AgentAuth,
    State(state): State<AppState>,
    Path(snapshot_id): Path<Uuid>,
    Json(body): Json<CommitFile>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, String)> {
    // Ensure snapshot belongs to this agent
    let ok: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM snapshots WHERE id = $1 AND agent_id = $2 AND status = 'running'",
    )
    .bind(snapshot_id)
    .bind(agent.agent_id)
    .fetch_optional(&state.db)
    .await
    .map_err(internal)?;
    if ok.is_none() {
        return Err((
            axum::http::StatusCode::NOT_FOUND,
            "snapshot not found".into(),
        ));
    }

    sqlx::query(
        r#"INSERT INTO snapshot_files (snapshot_id, relative_path, size_bytes, content_hash, manifest_json)
           VALUES ($1, $2, $3, $4, $5)
           ON CONFLICT (snapshot_id, relative_path) DO UPDATE
             SET size_bytes = EXCLUDED.size_bytes,
                 content_hash = EXCLUDED.content_hash,
                 manifest_json = EXCLUDED.manifest_json"#,
    )
    .bind(snapshot_id)
    .bind(&body.relative_path)
    .bind(body.size_bytes as i64)
    .bind(&body.content_hash)
    .bind(&body.manifest)
    .execute(&state.db)
    .await
    .map_err(internal)?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct FinishSnapshot {
    pub success: bool,
    pub error_message: Option<String>,
    pub raw_bytes: Option<u64>,
    pub stored_bytes: Option<u64>,
}

pub async fn finish_snapshot(
    agent: AgentAuth,
    State(state): State<AppState>,
    Path(snapshot_id): Path<Uuid>,
    Json(body): Json<FinishSnapshot>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, String)> {
    let file_count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM snapshot_files WHERE snapshot_id = $1")
            .bind(snapshot_id)
            .fetch_one(&state.db)
            .await
            .map_err(internal)?;

    let status = if body.success { "success" } else { "failed" };
    let err_msg = body.error_message.clone();
    sqlx::query(
        r#"UPDATE snapshots SET
             finished_at = NOW(),
             status = $2::job_status,
             file_count = $3,
             raw_bytes = COALESCE($4, raw_bytes),
             stored_bytes = COALESCE($5, stored_bytes),
             error_message = $6
           WHERE id = $1 AND agent_id = $7"#,
    )
    .bind(snapshot_id)
    .bind(status)
    .bind(file_count.0 as i32)
    .bind(body.raw_bytes.map(|v| v as i64))
    .bind(body.stored_bytes.map(|v| v as i64))
    .bind(&err_msg)
    .bind(agent.agent_id)
    .execute(&state.db)
    .await
    .map_err(internal)?;

    if body.success {
        let _ = sqlx::query("UPDATE agents SET last_backup_at = NOW() WHERE id = $1")
            .bind(agent.agent_id)
            .execute(&state.db)
            .await;
    } else {
        crate::notify::notify_webhook(
            &state.config.notify_webhook_url,
            "backup.failed",
            serde_json::json!({
                "agent_id": agent.agent_id,
                "snapshot_id": snapshot_id,
                "error": err_msg,
            }),
        )
        .await;
    }

    Ok(Json(serde_json::json!({ "ok": true, "status": status })))
}

fn internal(e: sqlx::Error) -> (axum::http::StatusCode, String) {
    (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}
