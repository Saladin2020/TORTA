use argon2::{
    password_hash::{
        rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
    },
    Argon2,
};
use axum::{
    extract::FromRequestParts,
    http::{header::AUTHORIZATION, request::Parts, StatusCode},
};
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use ldap3::{LdapConnAsync, Scope, SearchEntry};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::config::LdapConfig;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: Uuid,
    pub username: String,
    pub role: String,
    pub exp: i64,
}

#[derive(Debug, Clone)]
pub struct AuthUser {
    pub id: Uuid,
    pub username: String,
    pub role: String,
    pub must_change_password: bool,
}

impl AuthUser {
    pub fn is_admin(&self) -> bool {
        self.role == "admin"
    }

    pub fn is_auditor(&self) -> bool {
        self.role == "auditor" || self.is_admin()
    }
}

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("hash: {e}"))?
        .to_string();
    Ok(hash)
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

/// Agent tokens are looked up by BLAKE3 hex (never store plaintext once issued).
pub fn hash_agent_token(token: &str) -> String {
    blake3::hash(token.as_bytes()).to_hex().to_string()
}

pub fn issue_token(
    secret: &str,
    user_id: Uuid,
    username: &str,
    role: &str,
    ttl_hours: i64,
) -> anyhow::Result<String> {
    let hours = ttl_hours.clamp(1, 168);
    let claims = Claims {
        sub: user_id,
        username: username.to_string(),
        role: role.to_string(),
        exp: (Utc::now() + Duration::hours(hours)).timestamp(),
    };
    Ok(encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?)
}

pub fn decode_token(secret: &str, token: &str) -> anyhow::Result<Claims> {
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )?;
    Ok(data.claims)
}

/// Escape LDAP filter special chars (RFC 4515).
pub fn ldap_escape_filter(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\5c"),
            '*' => out.push_str("\\2a"),
            '(' => out.push_str("\\28"),
            ')' => out.push_str("\\29"),
            '\0' => out.push_str("\\00"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ldap_escape_blocks_injection() {
        assert_eq!(ldap_escape_filter("alice)(uid=*"), "alice\\29\\28uid=\\2a");
        assert_eq!(hash_agent_token("dice_abc").len(), 64);
    }
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = (StatusCode, String);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or((StatusCode::UNAUTHORIZED, "missing Authorization".into()))?;
        let token = header
            .strip_prefix("Bearer ")
            .ok_or((StatusCode::UNAUTHORIZED, "expected Bearer token".into()))?;
        let claims = decode_token(&state.config.jwt_secret, token)
            .map_err(|_| (StatusCode::UNAUTHORIZED, "invalid token".into()))?;

        let row = sqlx::query_as::<_, (bool, bool)>(
            "SELECT enabled, COALESCE(must_change_password, FALSE) FROM users WHERE id = $1",
        )
        .bind(claims.sub)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::UNAUTHORIZED, "user gone".into()))?;

        if !row.0 {
            return Err((StatusCode::FORBIDDEN, "user disabled".into()));
        }

        Ok(AuthUser {
            id: claims.sub,
            username: claims.username,
            role: claims.role,
            must_change_password: row.1,
        })
    }
}

#[derive(Debug, Clone)]
pub struct AgentAuth {
    pub agent_id: Uuid,
    pub user_id: Uuid,
}

impl FromRequestParts<AppState> for AgentAuth {
    type Rejection = (StatusCode, String);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get("X-Agent-Token")
            .and_then(|v| v.to_str().ok())
            .ok_or((StatusCode::UNAUTHORIZED, "missing X-Agent-Token".into()))?;

        let hashed = hash_agent_token(header);
        // Prefer hashed lookup; fall back to legacy plaintext row during migration.
        let row = sqlx::query_as::<_, (Uuid, Uuid, bool)>(
            r#"SELECT id, user_id, enabled FROM agents
               WHERE agent_token = $1 OR agent_token = $2
               LIMIT 1"#,
        )
        .bind(&hashed)
        .bind(header)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::UNAUTHORIZED, "unknown agent".into()))?;

        if !row.2 {
            return Err((StatusCode::FORBIDDEN, "agent disabled".into()));
        }

        // Opportunistically upgrade plaintext token to hash
        if header != hashed {
            let _ = sqlx::query(
                "UPDATE agents SET agent_token = $1 WHERE id = $2 AND agent_token = $3",
            )
            .bind(&hashed)
            .bind(row.0)
            .bind(header)
            .execute(&state.db)
            .await;
        }

        let _ = sqlx::query(
            "UPDATE agents SET last_seen_at = NOW(), status = 'online' WHERE id = $1",
        )
        .bind(row.0)
        .execute(&state.db)
        .await;

        Ok(AgentAuth {
            agent_id: row.0,
            user_id: row.1,
        })
    }
}

pub async fn authenticate_local(
    db: &PgPool,
    username: &str,
    password: &str,
) -> anyhow::Result<Option<(Uuid, String, String, bool)>> {
    let row = sqlx::query_as::<
        _,
        (
            Uuid,
            String,
            Option<String>,
            String,
            bool,
            bool,
            Option<chrono::DateTime<Utc>>,
        ),
    >(
        r#"SELECT id, username, password_hash, role::text, enabled,
                  COALESCE(must_change_password, FALSE), locked_until
           FROM users
           WHERE username = $1 AND auth_provider = 'local'"#,
    )
    .bind(username)
    .fetch_optional(db)
    .await?;

    let Some((id, uname, hash, role, enabled, must_change, locked_until)) = row else {
        return Ok(None);
    };
    if !enabled {
        return Ok(None);
    }
    if let Some(until) = locked_until {
        if until > Utc::now() {
            anyhow::bail!("account temporarily locked until {until}");
        }
    }
    let Some(hash) = hash else {
        return Ok(None);
    };
    if !verify_password(password, &hash) {
        let _ = sqlx::query(
            r#"UPDATE users SET
                 failed_login_count = failed_login_count + 1,
                 locked_until = CASE
                   WHEN failed_login_count + 1 >= 10 THEN NOW() + INTERVAL '15 minutes'
                   ELSE locked_until
                 END
               WHERE id = $1"#,
        )
        .bind(id)
        .execute(db)
        .await;
        return Ok(None);
    }

    let _ = sqlx::query(
        "UPDATE users SET failed_login_count = 0, locked_until = NULL WHERE id = $1",
    )
    .bind(id)
    .execute(db)
    .await;

    Ok(Some((id, uname, role, must_change)))
}

pub async fn authenticate_ldap(
    cfg: &LdapConfig,
    db: &PgPool,
    username: &str,
    password: &str,
) -> anyhow::Result<Option<(Uuid, String, String, bool)>> {
    if !cfg.enabled {
        return Ok(None);
    }
    if password.is_empty() {
        return Ok(None);
    }

    let (conn, mut ldap) = LdapConnAsync::new(&cfg.url).await?;
    ldap3::drive!(conn);
    ldap.simple_bind(&cfg.bind_dn, &cfg.bind_password)
        .await?
        .success()?;

    let safe_user = ldap_escape_filter(username);
    let filter = cfg.user_filter.replace("{username}", &safe_user);
    let (rs, _) = ldap
        .search(
            &cfg.base_dn,
            Scope::Subtree,
            &filter,
            vec!["dn", &cfg.username_attr],
        )
        .await?
        .success()?;

    let entry = rs.into_iter().next().map(SearchEntry::construct);
    let Some(entry) = entry else {
        return Ok(None);
    };
    let user_dn = entry.dn;

    let (conn2, mut ldap2) = LdapConnAsync::new(&cfg.url).await?;
    ldap3::drive!(conn2);
    if ldap2.simple_bind(&user_dn, password).await?.success().is_err() {
        return Ok(None);
    }

    let row = sqlx::query_as::<_, (Uuid, String, String)>(
        r#"INSERT INTO users (username, display_name, auth_provider, ldap_dn, role)
           VALUES ($1, $1, 'ldap', $2, 'user')
           ON CONFLICT (username) DO UPDATE
             SET ldap_dn = EXCLUDED.ldap_dn, updated_at = NOW()
           RETURNING id, username, role::text"#,
    )
    .bind(username)
    .bind(&user_dn)
    .fetch_one(db)
    .await?;

    let enabled: (bool,) = sqlx::query_as("SELECT enabled FROM users WHERE id = $1")
        .bind(row.0)
        .fetch_one(db)
        .await?;
    if !enabled.0 {
        return Ok(None);
    }

    Ok(Some((row.0, row.1, row.2, false)))
}
