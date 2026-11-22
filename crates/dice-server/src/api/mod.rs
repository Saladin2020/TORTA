pub mod admin;
pub mod agent;
pub mod auth_routes;
pub mod downloads;
pub mod portal;
pub mod upload;
pub mod ws;

use axum::{
    extract::State,
    routing::{get, post, put},
    Json, Router,
};

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/auth/login", post(auth_routes::login))
        .route("/api/auth/me", get(auth_routes::me))
        .route("/api/auth/change-password", post(auth_routes::change_password))
        // Admin
        .route("/api/admin/storage", get(admin::list_storage).post(admin::create_storage))
        .route("/api/admin/storage/{id}", put(admin::update_storage))
        .route("/api/admin/policies", get(admin::list_policies).post(admin::create_policy))
        .route("/api/admin/policies/{id}", put(admin::update_policy))
        .route("/api/admin/agents", get(admin::list_agents).post(admin::create_agent))
        .route("/api/admin/agents/{id}", put(admin::update_agent))
        .route("/api/admin/agents/{id}/run-now", post(admin::run_now))
        .route("/api/admin/users", get(admin::list_users).post(admin::create_user))
        .route("/api/admin/users/{id}", put(admin::update_user))
        .route("/api/admin/stats", get(admin::stats))
        .route("/api/admin/gc", post(admin::run_gc))
        .route("/api/admin/audit", get(admin::list_audit))
        // Portal (end user)
        .route("/api/portal/files", get(portal::list_path_files))
        .route("/api/portal/store", get(portal::list_unique_store))
        .route("/api/portal/snapshots", get(portal::list_snapshots))
        .route("/api/portal/snapshots/{id}/files", get(portal::list_files))
        .route(
            "/api/portal/snapshots/{id}/files/{file_id}/download",
            get(portal::download_file),
        )
        .route(
            "/api/portal/snapshots/{id}/files/{file_id}/verify",
            post(portal::verify_file),
        )
        .route(
            "/api/portal/snapshots/{id}/zip",
            get(portal::download_snapshot_zip),
        )
        .route(
            "/api/portal/snapshots/{id}/trash",
            post(portal::soft_delete_snapshot),
        )
        // Web selective upload
        .route("/api/portal/upload", post(upload::upload_files))
        // Agent downloads
        .route("/api/portal/agent/downloads", get(downloads::list_agent_downloads))
        .route(
            "/api/portal/agent/download/{id}",
            get(downloads::download_agent),
        )
        // Agent protocol
        .route("/api/agent/policy", get(agent::get_policy))
        .route("/api/agent/heartbeat", post(agent::heartbeat))
        .route("/api/agent/chunks/missing", post(agent::missing_chunks))
        .route("/api/agent/chunks", post(agent::upload_chunk))
        .route("/api/agent/snapshots", post(agent::begin_snapshot))
        .route("/api/agent/snapshots/{id}/files", post(agent::commit_file))
        .route("/api/agent/snapshots/{id}/finish", post(agent::finish_snapshot))
        .route("/api/agent/ws", get(ws::agent_ws))
}

async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    let db_ok = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.db)
        .await
        .is_ok();

    let store_path = &state.config.store_path;
    let store_writable = {
        let probe = store_path.join(".health_write");
        match std::fs::write(&probe, b"ok") {
            Ok(()) => {
                let _ = std::fs::remove_file(&probe);
                true
            }
            Err(_) => false,
        }
    };

    let disk_free = fs_free_bytes(store_path);
    let encrypted = !state.config.store_key_hex.trim().is_empty();
    let ok = db_ok && store_writable;

    Json(serde_json::json!({
        "ok": ok,
        "service": "dice-server",
        "db": db_ok,
        "store_writable": store_writable,
        "store_path": store_path.display().to_string(),
        "store_encrypted": encrypted,
        "disk_free_bytes": disk_free,
        "production": state.config.production,
    }))
}

fn fs_free_bytes(path: &std::path::Path) -> Option<u64> {
    // Best-effort: walk up until path exists
    let mut p = path.to_path_buf();
    loop {
        if p.exists() {
            break;
        }
        if !p.pop() {
            return None;
        }
    }
    // No portable free-space crate; report None rather than fake numbers.
    // Ops can use OS tools; health still checks writability.
    let _ = p;
    None
}
