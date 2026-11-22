use axum::{
    extract::{Path, State},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::audit;
use crate::auth::{hash_agent_token, hash_password, AuthUser};
use crate::state::AppState;

fn require_admin(user: &AuthUser) -> Result<(), (axum::http::StatusCode, String)> {
    if user.is_admin() {
        Ok(())
    } else {
        Err((axum::http::StatusCode::FORBIDDEN, "admin only".into()))
    }
}

fn require_staff(user: &AuthUser) -> Result<(), (axum::http::StatusCode, String)> {
    if user.is_auditor() {
        Ok(())
    } else {
        Err((axum::http::StatusCode::FORBIDDEN, "admin/auditor only".into()))
    }
}

#[derive(Serialize, sqlx::FromRow)]
pub struct StorageRow {
    pub id: Uuid,
    pub name: String,
    pub path: String,
    pub is_default: bool,
    pub quota_bytes: Option<i64>,
    pub used_bytes: i64,
    pub notes: String,
}

pub async fn list_storage(
    user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<StorageRow>>, (axum::http::StatusCode, String)> {
    require_staff(&user)?;
    let rows = sqlx::query_as::<_, StorageRow>(
        "SELECT id, name, path, is_default, quota_bytes, used_bytes, notes FROM storage_targets ORDER BY name",
    )
    .fetch_all(&state.db)
    .await
    .map_err(internal)?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
pub struct CreateStorage {
    pub name: String,
    pub path: String,
    pub is_default: Option<bool>,
    pub quota_bytes: Option<i64>,
    pub notes: Option<String>,
}

pub async fn create_storage(
    user: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<CreateStorage>,
) -> Result<Json<StorageRow>, (axum::http::StatusCode, String)> {
    require_admin(&user)?;
    std::fs::create_dir_all(&body.path).map_err(|e| {
        (
            axum::http::StatusCode::BAD_REQUEST,
            format!("cannot create path: {e}"),
        )
    })?;
    dice_core::init_store(&body.path).map_err(|e| {
        (
            axum::http::StatusCode::BAD_REQUEST,
            format!("init store: {e}"),
        )
    })?;

    let is_default = body.is_default.unwrap_or(false);
    if is_default {
        let _ = sqlx::query("UPDATE storage_targets SET is_default = FALSE")
            .execute(&state.db)
            .await;
    }

    let row = sqlx::query_as::<_, StorageRow>(
        r#"INSERT INTO storage_targets (name, path, is_default, quota_bytes, notes)
           VALUES ($1, $2, $3, $4, $5)
           RETURNING id, name, path, is_default, quota_bytes, used_bytes, notes"#,
    )
    .bind(&body.name)
    .bind(&body.path)
    .bind(is_default)
    .bind(body.quota_bytes)
    .bind(body.notes.unwrap_or_default())
    .fetch_one(&state.db)
    .await
    .map_err(internal)?;
    Ok(Json(row))
}

#[derive(Serialize, sqlx::FromRow)]
pub struct PolicyRow {
    pub id: Uuid,
    pub name: String,
    pub enabled: bool,
    pub schedule_cron: String,
    pub paths_json: serde_json::Value,
    pub excludes_json: serde_json::Value,
    pub retention_days: i32,
    pub bandwidth_bps: Option<i64>,
    pub storage_target_id: Option<Uuid>,
}

pub async fn list_policies(
    user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<PolicyRow>>, (axum::http::StatusCode, String)> {
    require_staff(&user)?;
    let rows = sqlx::query_as::<_, PolicyRow>(
        r#"SELECT id, name, enabled, schedule_cron, paths_json, excludes_json,
                  retention_days, bandwidth_bps, storage_target_id
           FROM policies ORDER BY name"#,
    )
    .fetch_all(&state.db)
    .await
    .map_err(internal)?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
pub struct CreatePolicy {
    pub name: String,
    pub schedule_cron: Option<String>,
    pub paths: Vec<String>,
    pub excludes: Option<Vec<String>>,
    pub retention_days: Option<i32>,
    pub bandwidth_bps: Option<i64>,
    pub storage_target_id: Option<Uuid>,
    pub enabled: Option<bool>,
}

pub async fn create_policy(
    user: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<CreatePolicy>,
) -> Result<Json<PolicyRow>, (axum::http::StatusCode, String)> {
    require_admin(&user)?;
    let row = sqlx::query_as::<_, PolicyRow>(
        r#"INSERT INTO policies
            (name, enabled, schedule_cron, paths_json, excludes_json, retention_days, bandwidth_bps, storage_target_id)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
           RETURNING id, name, enabled, schedule_cron, paths_json, excludes_json,
                     retention_days, bandwidth_bps, storage_target_id"#,
    )
    .bind(&body.name)
    .bind(body.enabled.unwrap_or(true))
    .bind(body.schedule_cron.unwrap_or_else(|| "0 2 * * *".into()))
    .bind(serde_json::to_value(&body.paths).unwrap())
    .bind(serde_json::to_value(body.excludes.unwrap_or_default()).unwrap())
    .bind(body.retention_days.unwrap_or(30))
    .bind(body.bandwidth_bps)
    .bind(body.storage_target_id)
    .fetch_one(&state.db)
    .await
    .map_err(internal)?;
    Ok(Json(row))
}

#[derive(Deserialize)]
pub struct UpdatePolicy {
    pub enabled: Option<bool>,
    pub schedule_cron: Option<String>,
    pub paths: Option<Vec<String>>,
    pub excludes: Option<Vec<String>>,
    pub retention_days: Option<i32>,
    pub bandwidth_bps: Option<i64>,
    pub storage_target_id: Option<Uuid>,
}

pub async fn update_policy(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdatePolicy>,
) -> Result<Json<PolicyRow>, (axum::http::StatusCode, String)> {
    require_admin(&user)?;
    let row = sqlx::query_as::<_, PolicyRow>(
        r#"UPDATE policies SET
             enabled = COALESCE($2, enabled),
             schedule_cron = COALESCE($3, schedule_cron),
             paths_json = COALESCE($4, paths_json),
             excludes_json = COALESCE($5, excludes_json),
             retention_days = COALESCE($6, retention_days),
             bandwidth_bps = COALESCE($7, bandwidth_bps),
             storage_target_id = COALESCE($8, storage_target_id),
             updated_at = NOW()
           WHERE id = $1
           RETURNING id, name, enabled, schedule_cron, paths_json, excludes_json,
                     retention_days, bandwidth_bps, storage_target_id"#,
    )
    .bind(id)
    .bind(body.enabled)
    .bind(body.schedule_cron)
    .bind(body.paths.map(|p| serde_json::to_value(p).unwrap()))
    .bind(body.excludes.map(|p| serde_json::to_value(p).unwrap()))
    .bind(body.retention_days)
    .bind(body.bandwidth_bps)
    .bind(body.storage_target_id)
    .fetch_one(&state.db)
    .await
    .map_err(internal)?;
    Ok(Json(row))
}

#[derive(Serialize, sqlx::FromRow)]
pub struct AgentRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub policy_id: Option<Uuid>,
    pub name: String,
    pub hostname: String,
    pub os: String,
    pub enabled: bool,
    pub local_enabled: bool,
    pub status: String,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub username: String,
}

pub async fn list_agents(
    user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<AgentRow>>, (axum::http::StatusCode, String)> {
    require_staff(&user)?;
    let rows = sqlx::query_as::<_, AgentRow>(
        r#"SELECT a.id, a.user_id, a.policy_id, a.name, a.hostname, a.os,
                  a.enabled, a.local_enabled, a.status::text, a.last_seen_at, u.username
           FROM agents a JOIN users u ON u.id = a.user_id
           ORDER BY a.name"#,
    )
    .fetch_all(&state.db)
    .await
    .map_err(internal)?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
pub struct CreateAgent {
    pub user_id: Uuid,
    pub name: String,
    pub policy_id: Option<Uuid>,
}

#[derive(Serialize)]
pub struct CreateAgentResponse {
    pub agent: AgentRow,
    pub agent_token: String,
}

pub async fn create_agent(
    user: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<CreateAgent>,
) -> Result<Json<CreateAgentResponse>, (axum::http::StatusCode, String)> {
    require_admin(&user)?;
    let token = format!("dice_{}", Uuid::new_v4().simple());
    let token_hash = hash_agent_token(&token);
    let row = sqlx::query_as::<_, AgentRow>(
        r#"WITH inserted AS (
             INSERT INTO agents (user_id, policy_id, name, agent_token)
             VALUES ($1, $2, $3, $4)
             RETURNING id, user_id, policy_id, name, hostname, os, enabled, local_enabled, status, last_seen_at
           )
           SELECT i.id, i.user_id, i.policy_id, i.name, i.hostname, i.os,
                  i.enabled, i.local_enabled, i.status::text, i.last_seen_at, u.username
           FROM inserted i JOIN users u ON u.id = i.user_id"#,
    )
    .bind(body.user_id)
    .bind(body.policy_id)
    .bind(&body.name)
    .bind(&token_hash)
    .fetch_one(&state.db)
    .await
    .map_err(internal)?;
    audit::write(
        &state.db,
        Some(user.id),
        "admin.agent_create",
        serde_json::json!({ "agent_id": row.id, "name": body.name }),
    )
    .await;
    Ok(Json(CreateAgentResponse {
        agent: row,
        agent_token: token,
    }))
}

#[derive(Deserialize)]
pub struct UpdateAgent {
    pub enabled: Option<bool>,
    pub policy_id: Option<Uuid>,
    pub name: Option<String>,
}

pub async fn update_agent(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateAgent>,
) -> Result<Json<AgentRow>, (axum::http::StatusCode, String)> {
    require_admin(&user)?;
    let row = sqlx::query_as::<_, AgentRow>(
        r#"UPDATE agents SET
             enabled = COALESCE($2, enabled),
             policy_id = COALESCE($3, policy_id),
             name = COALESCE($4, name)
           WHERE id = $1
           RETURNING id, user_id, policy_id, name, hostname, os, enabled, local_enabled,
                     status::text, last_seen_at,
                     (SELECT username FROM users WHERE id = agents.user_id) AS username"#,
    )
    .bind(id)
    .bind(body.enabled)
    .bind(body.policy_id)
    .bind(body.name)
    .fetch_one(&state.db)
    .await
    .map_err(internal)?;
    Ok(Json(row))
}

#[derive(Serialize, sqlx::FromRow)]
pub struct UserRow {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub role: String,
    pub auth_provider: String,
    pub enabled: bool,
}

pub async fn list_users(
    user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<UserRow>>, (axum::http::StatusCode, String)> {
    require_staff(&user)?;
    let rows = sqlx::query_as::<_, UserRow>(
        r#"SELECT id, username, display_name, role::text, auth_provider::text, enabled
           FROM users ORDER BY username"#,
    )
    .fetch_all(&state.db)
    .await
    .map_err(internal)?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
pub struct CreateUser {
    pub username: String,
    pub password: String,
    pub display_name: Option<String>,
    pub role: Option<String>,
}

pub async fn create_user(
    user: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<CreateUser>,
) -> Result<Json<UserRow>, (axum::http::StatusCode, String)> {
    require_admin(&user)?;
    let hash = hash_password(&body.password)
        .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let role = body.role.unwrap_or_else(|| "user".into());
    if !matches!(role.as_str(), "user" | "admin" | "auditor") {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            "role must be user|admin|auditor".into(),
        ));
    }
    if body.password.len() < 10 {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            "password must be at least 10 characters".into(),
        ));
    }
    let row = sqlx::query_as::<_, UserRow>(
        r#"INSERT INTO users (username, display_name, role, auth_provider, password_hash, must_change_password)
           VALUES ($1, $2, $3::user_role, 'local', $4, TRUE)
           RETURNING id, username, display_name, role::text, auth_provider::text, enabled"#,
    )
    .bind(&body.username)
    .bind(body.display_name.unwrap_or_else(|| body.username.clone()))
    .bind(&role)
    .bind(hash)
    .fetch_one(&state.db)
    .await
    .map_err(internal)?;
    audit::write(
        &state.db,
        Some(user.id),
        "admin.user_create",
        serde_json::json!({ "user_id": row.id, "username": row.username, "role": role }),
    )
    .await;
    Ok(Json(row))
}

#[derive(Serialize)]
pub struct Stats {
    pub users: i64,
    pub agents: i64,
    pub snapshots: i64,
    pub store_path: String,
    pub store_used_bytes: i64,
    pub store_quota_bytes: Option<i64>,
}

pub async fn stats(
    user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Stats>, (axum::http::StatusCode, String)> {
    require_staff(&user)?;
    let users: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(&state.db)
        .await
        .map_err(internal)?;
    let agents: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM agents")
        .fetch_one(&state.db)
        .await
        .map_err(internal)?;
    let snapshots: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM snapshots")
        .fetch_one(&state.db)
        .await
        .map_err(internal)?;
    let store: (i64, Option<i64>) = sqlx::query_as(
        r#"SELECT COALESCE(used_bytes, 0), quota_bytes
           FROM storage_targets
           WHERE is_default = TRUE
           ORDER BY created_at
           LIMIT 1"#,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(internal)?
    .unwrap_or((0, None));
    Ok(Json(Stats {
        users: users.0,
        agents: agents.0,
        snapshots: snapshots.0,
        store_path: state.config.store_path.display().to_string(),
        store_used_bytes: store.0,
        store_quota_bytes: store.1,
    }))
}

pub async fn run_now(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, String)> {
    require_admin(&user)?;
    let updated = sqlx::query(
        r#"UPDATE agents SET run_now = TRUE
           WHERE id = $1 AND enabled = TRUE"#,
    )
    .bind(id)
    .execute(&state.db)
    .await
    .map_err(internal)?
    .rows_affected();
    if updated == 0 {
        return Err((
            axum::http::StatusCode::NOT_FOUND,
            "agent not found or disabled".into(),
        ));
    }
    let _ = sqlx::query(
        r#"INSERT INTO audit_log (actor_user_id, action, detail)
           VALUES ($1, 'agent.run_now', jsonb_build_object('agent_id', $2::text))"#,
    )
    .bind(user.id)
    .bind(id)
    .execute(&state.db)
    .await;
    state
        .hub
        .notify(id, r#"{"type":"run_now"}"#)
        .await;
    Ok(Json(serde_json::json!({ "ok": true, "agent_id": id })))
}

pub async fn run_gc(
    user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, String)> {
    require_admin(&user)?;
    let report = crate::retention::run_retention(&state)
        .await
        .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    audit::write(
        &state.db,
        Some(user.id),
        "admin.gc",
        serde_json::json!({ "message": report.message }),
    )
    .await;
    Ok(Json(serde_json::json!({
        "ok": true,
        "snapshots_deleted": report.snapshots_deleted,
        "chunks_deleted": report.chunks_deleted,
        "bytes_reclaimed": report.bytes_reclaimed,
        "stale_failed": report.stale_failed,
        "message": report.message,
    })))
}

#[derive(Serialize, sqlx::FromRow)]
pub struct AuditRow {
    pub id: i64,
    pub actor_user_id: Option<Uuid>,
    pub action: String,
    pub detail: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub actor_username: Option<String>,
}

pub async fn list_audit(
    user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<AuditRow>>, (axum::http::StatusCode, String)> {
    if !user.is_auditor() {
        return Err((axum::http::StatusCode::FORBIDDEN, "admin/auditor only".into()));
    }
    let rows = sqlx::query_as::<_, AuditRow>(
        r#"SELECT a.id, a.actor_user_id, a.action, a.detail, a.created_at,
                  u.username AS actor_username
           FROM audit_log a
           LEFT JOIN users u ON u.id = a.actor_user_id
           ORDER BY a.created_at DESC
           LIMIT 200"#,
    )
    .fetch_all(&state.db)
    .await
    .map_err(internal)?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
pub struct UpdateStorage {
    pub quota_bytes: Option<i64>,
    pub notes: Option<String>,
}

pub async fn update_storage(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateStorage>,
) -> Result<Json<StorageRow>, (axum::http::StatusCode, String)> {
    require_admin(&user)?;
    let clear_quota = body.quota_bytes == Some(-1);
    let quota = if clear_quota {
        None
    } else {
        body.quota_bytes
    };
    let row = sqlx::query_as::<_, StorageRow>(
        r#"UPDATE storage_targets SET
             quota_bytes = CASE
               WHEN $2::bool THEN NULL
               WHEN $3::bigint IS NOT NULL THEN $3
               ELSE quota_bytes
             END,
             notes = COALESCE($4, notes)
           WHERE id = $1
           RETURNING id, name, path, is_default, quota_bytes, used_bytes, notes"#,
    )
    .bind(id)
    .bind(clear_quota)
    .bind(quota)
    .bind(body.notes)
    .fetch_one(&state.db)
    .await
    .map_err(internal)?;
    Ok(Json(row))
}

#[derive(Deserialize)]
pub struct UpdateUserQuota {
    pub quota_bytes: Option<i64>,
    pub enabled: Option<bool>,
}

pub async fn update_user(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateUserQuota>,
) -> Result<Json<UserRow>, (axum::http::StatusCode, String)> {
    require_admin(&user)?;
    // Clear quota when explicitly sending null via separate flag is awkward in JSON;
    // use negative sentinel (-1) to clear, or omit to leave unchanged.
    let clear_quota = body.quota_bytes == Some(-1);
    let quota = if clear_quota {
        None
    } else {
        body.quota_bytes
    };
    let row = sqlx::query_as::<_, UserRow>(
        r#"UPDATE users SET
             quota_bytes = CASE
               WHEN $2::bool THEN NULL
               WHEN $3::bigint IS NOT NULL THEN $3
               ELSE quota_bytes
             END,
             enabled = COALESCE($4, enabled),
             updated_at = NOW()
           WHERE id = $1
           RETURNING id, username, display_name, role::text, auth_provider::text, enabled"#,
    )
    .bind(id)
    .bind(clear_quota)
    .bind(quota)
    .bind(body.enabled)
    .fetch_one(&state.db)
    .await
    .map_err(internal)?;
    Ok(Json(row))
}

fn internal(e: sqlx::Error) -> (axum::http::StatusCode, String) {
    (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}
