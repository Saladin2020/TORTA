use axum::{
    extract::{ConnectInfo, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

use crate::audit;
use crate::auth::{
    authenticate_ldap, authenticate_local, hash_password, issue_token, verify_password, AuthUser,
};
use crate::state::AppState;

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    #[serde(default = "default_provider")]
    pub provider: String,
}

fn default_provider() -> String {
    "auto".into()
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub username: String,
    pub role: String,
    pub provider: String,
    pub must_change_password: bool,
}

pub async fn login(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, (axum::http::StatusCode, String)> {
    let ip = addr.ip().to_string();
    let key = format!("{}|{}", body.username.to_lowercase(), ip);

    state
        .login_limiter
        .check_and_hit(&key)
        .map_err(|e| (axum::http::StatusCode::TOO_MANY_REQUESTS, e))?;

    let try_local = body.provider == "local" || body.provider == "auto";
    let try_ldap =
        body.provider == "ldap" || (body.provider == "auto" && state.config.ldap.enabled);

    let mut authed: Option<(uuid::Uuid, String, String, bool, &str)> = None;

    if try_local {
        match authenticate_local(&state.db, &body.username, &body.password).await {
            Ok(Some((id, username, role, must_change))) => {
                authed = Some((id, username, role, must_change, "local"));
            }
            Ok(None) => {}
            Err(e) => {
                return Err((axum::http::StatusCode::FORBIDDEN, e.to_string()));
            }
        }
    }

    if authed.is_none() && try_ldap {
        if let Some((id, username, role, must_change)) =
            authenticate_ldap(&state.config.ldap, &state.db, &body.username, &body.password)
                .await
                .map_err(|e| (axum::http::StatusCode::BAD_GATEWAY, e.to_string()))?
        {
            authed = Some((id, username, role, must_change, "ldap"));
        }
    }

    let Some((id, username, role, must_change, provider)) = authed else {
        let _ = sqlx::query(
            "INSERT INTO login_attempts (username, ip, success) VALUES ($1, $2, FALSE)",
        )
        .bind(&body.username)
        .bind(&ip)
        .execute(&state.db)
        .await;
        audit::write(
            &state.db,
            None,
            "auth.login_failed",
            serde_json::json!({ "username": body.username, "ip": ip }),
        )
        .await;
        return Err((
            axum::http::StatusCode::UNAUTHORIZED,
            "invalid credentials".into(),
        ));
    };

    state.login_limiter.clear(&key);
    let _ = sqlx::query("INSERT INTO login_attempts (username, ip, success) VALUES ($1, $2, TRUE)")
        .bind(&username)
        .bind(&ip)
        .execute(&state.db)
        .await;

    let token = issue_token(
        &state.config.jwt_secret,
        id,
        &username,
        &role,
        state.config.jwt_ttl_hours,
    )
    .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    audit::write(
        &state.db,
        Some(id),
        "auth.login",
        serde_json::json!({ "provider": provider, "ip": ip }),
    )
    .await;

    Ok(Json(LoginResponse {
        token,
        username,
        role,
        provider: provider.into(),
        must_change_password: must_change,
    }))
}

pub async fn me(user: AuthUser) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "id": user.id,
        "username": user.username,
        "role": user.role,
        "must_change_password": user.must_change_password,
    }))
}

#[derive(Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

pub async fn change_password(
    user: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<ChangePasswordRequest>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, String)> {
    if body.new_password.len() < 10 {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            "new password must be at least 10 characters".into(),
        ));
    }

    let row = sqlx::query_as::<_, (Option<String>, String)>(
        "SELECT password_hash, auth_provider::text FROM users WHERE id = $1",
    )
    .bind(user.id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if row.1 != "local" {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            "LDAP accounts change password in the directory".into(),
        ));
    }
    let Some(hash) = row.0 else {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            "no local password set".into(),
        ));
    };
    if !verify_password(&body.current_password, &hash) {
        return Err((
            axum::http::StatusCode::UNAUTHORIZED,
            "current password incorrect".into(),
        ));
    }

    let new_hash = hash_password(&body.new_password)
        .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    sqlx::query(
        r#"UPDATE users SET
             password_hash = $2,
             must_change_password = FALSE,
             password_changed_at = NOW(),
             failed_login_count = 0,
             locked_until = NULL,
             updated_at = NOW()
           WHERE id = $1"#,
    )
    .bind(user.id)
    .bind(new_hash)
    .execute(&state.db)
    .await
    .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    audit::write(
        &state.db,
        Some(user.id),
        "auth.password_changed",
        serde_json::json!({}),
    )
    .await;

    Ok(Json(serde_json::json!({ "ok": true })))
}
