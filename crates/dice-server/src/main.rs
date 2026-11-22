mod api;
mod audit;
mod auth;
mod config;
mod hub;
mod notify;
mod quota;
mod rate_limit;
mod retention;
mod state;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::http::{HeaderValue, Method};
use axum::Router;
use clap::Parser;
use sqlx::postgres::PgPoolOptions;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use crate::auth::hash_password;
use crate::config::Config;
use crate::rate_limit::LoginLimiter;
use crate::state::AppState;

#[derive(Parser, Debug)]
#[command(name = "dice-server", about = "TORTA Dice org backup server")]
struct Args {
    /// Path to dice-server.toml
    #[arg(long, short = 'c')]
    config: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let args = Args::parse();
    let config = Config::load(args.config.as_ref())?;
    dice_core::init_store(&config.store_path)?;
    if !config.store_key_hex.trim().is_empty() {
        let cipher = dice_core::StoreCipher::from_hex(&config.store_key_hex)?;
        dice_core::set_store_cipher(Some(cipher));
        tracing::info!("store at-rest encryption enabled (ChaCha20-Poly1305)");
    } else {
        tracing::warn!("store_key_hex empty — chunks stored in plaintext (dev only)");
    }

    let db = PgPoolOptions::new()
        .max_connections(20)
        .connect(&config.database_url)
        .await?;

    sqlx::migrate!("../../migrations").run(&db).await?;
    bootstrap_admin(&db).await?;
    ensure_default_storage(&db, &config).await?;

    let login_limiter = Arc::new(LoginLimiter::new(
        config.login_max_attempts,
        config.login_window_secs,
    ));

    let state = AppState {
        db,
        config: Arc::new(config.clone()),
        hub: crate::hub::AgentHub::new(),
        login_limiter,
    };

    retention::spawn_retention_loop(state.clone());

    let cors = build_cors(&config)?;
    let mut app = Router::new()
        .merge(api::router())
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    if let Some(web_root) = &config.web_root {
        if web_root.exists() {
            tracing::info!("serving web UI from {}", web_root.display());
            app = app.fallback_service(ServeDir::new(web_root));
        }
    }

    let listener = tokio::net::TcpListener::bind(&config.listen).await?;
    tracing::info!(
        "dice-server listening on http://{} (production={})",
        config.listen,
        config.production
    );
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

fn build_cors(config: &Config) -> anyhow::Result<CorsLayer> {
    if config.cors_origins.is_empty() {
        tracing::warn!("cors_origins empty — allowing any origin (dev only)");
        return Ok(CorsLayer::new()
            .allow_origin(tower_http::cors::Any)
            .allow_methods([
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::DELETE,
                Method::OPTIONS,
            ])
            .allow_headers(tower_http::cors::Any));
    }

    let origins: Vec<HeaderValue> = config
        .cors_origins
        .iter()
        .map(|o| HeaderValue::from_str(o))
        .collect::<Result<_, _>>()?;

    Ok(CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers(tower_http::cors::Any)
        .allow_credentials(true))
}

async fn bootstrap_admin(db: &sqlx::PgPool) -> anyhow::Result<()> {
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users").fetch_one(db).await?;
    if count.0 > 0 {
        return Ok(());
    }

    let bootstrap_pw = std::env::var("DICE_BOOTSTRAP_ADMIN_PASSWORD")
        .unwrap_or_else(|_| "admin123".into());
    let must_change = bootstrap_pw == "admin123";
    if must_change {
        tracing::warn!(
            "created default admin / admin123 — MUST change password (or set DICE_BOOTSTRAP_ADMIN_PASSWORD)"
        );
    } else {
        tracing::info!("created bootstrap admin from DICE_BOOTSTRAP_ADMIN_PASSWORD");
    }

    let hash = hash_password(&bootstrap_pw)?;
    sqlx::query(
        r#"INSERT INTO users (username, display_name, role, auth_provider, password_hash, must_change_password)
           VALUES ('admin', 'Administrator', 'admin', 'local', $1, $2)"#,
    )
    .bind(hash)
    .bind(must_change)
    .execute(db)
    .await?;
    Ok(())
}

async fn ensure_default_storage(db: &sqlx::PgPool, config: &Config) -> anyhow::Result<()> {
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM storage_targets")
        .fetch_one(db)
        .await?;
    if count.0 > 0 {
        return Ok(());
    }
    let path = config.store_path.display().to_string();
    sqlx::query(
        r#"INSERT INTO storage_targets (name, path, is_default, notes)
           VALUES ('default', $1, TRUE, 'org-wide shared Dice store')"#,
    )
    .bind(&path)
    .execute(db)
    .await?;

    sqlx::query(
        r#"INSERT INTO policies (name, enabled, schedule_cron, paths_json, excludes_json, retention_days)
           VALUES (
             'default-nightly',
             TRUE,
             '0 2 * * *',
             '["Documents","Desktop"]'::jsonb,
             '[".git","node_modules","target"]'::jsonb,
             30
           )
           ON CONFLICT (name) DO NOTHING"#,
    )
    .execute(db)
    .await?;
    Ok(())
}
