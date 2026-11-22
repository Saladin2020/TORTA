use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub listen: String,
    pub database_url: String,
    pub jwt_secret: String,
    pub store_path: PathBuf,
    pub web_root: Option<PathBuf>,
    /// 64 hex chars (32 bytes). Required when `production = true`.
    #[serde(default)]
    pub store_key_hex: String,
    /// Fail boot on weak secrets / missing store key.
    #[serde(default)]
    pub production: bool,
    /// Allowed browser origins. Empty = allow any (dev only).
    #[serde(default)]
    pub cors_origins: Vec<String>,
    /// JWT lifetime in hours (default 8).
    #[serde(default = "default_jwt_hours")]
    pub jwt_ttl_hours: i64,
    /// Max failed logins per username+ip window.
    #[serde(default = "default_login_max")]
    pub login_max_attempts: u32,
    /// Login rate-limit window seconds.
    #[serde(default = "default_login_window")]
    pub login_window_secs: u64,
    /// Hours after which `running` snapshots are marked failed.
    #[serde(default = "default_stale_hours")]
    pub stale_snapshot_hours: i64,
    /// Optional SMTP / webhook notify URL (POST JSON on backup failure).
    #[serde(default)]
    pub notify_webhook_url: String,
    /// Directory with packaged agent binaries (see deploy/publish-agent.*).
    #[serde(default = "default_agent_dist")]
    pub agent_dist_dir: PathBuf,
    pub ldap: LdapConfig,
}

fn default_agent_dist() -> PathBuf {
    PathBuf::from("./dist/agents")
}

fn default_jwt_hours() -> i64 {
    8
}
fn default_login_max() -> u32 {
    8
}
fn default_login_window() -> u64 {
    300
}
fn default_stale_hours() -> i64 {
    6
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LdapConfig {
    pub enabled: bool,
    pub url: String,
    pub bind_dn: String,
    pub bind_password: String,
    pub base_dn: String,
    pub user_filter: String,
    pub username_attr: String,
}

impl Default for LdapConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            url: "ldaps://dc.example.local:636".into(),
            bind_dn: "CN=BackupSvc,OU=Service,DC=example,DC=local".into(),
            bind_password: String::new(),
            base_dn: "DC=example,DC=local".into(),
            user_filter: "(&(objectClass=user)(sAMAccountName={username}))".into(),
            username_attr: "sAMAccountName".into(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen: "0.0.0.0:8080".into(),
            database_url: "postgres://dice:dice@127.0.0.1:5432/dice".into(),
            jwt_secret: "change-me-in-production".into(),
            store_path: PathBuf::from("./data/dice-store"),
            web_root: Some(PathBuf::from("./apps/web/dist")),
            store_key_hex: String::new(),
            production: false,
            cors_origins: Vec::new(),
            jwt_ttl_hours: default_jwt_hours(),
            login_max_attempts: default_login_max(),
            login_window_secs: default_login_window(),
            stale_snapshot_hours: default_stale_hours(),
            notify_webhook_url: String::new(),
            agent_dist_dir: default_agent_dist(),
            ldap: LdapConfig::default(),
        }
    }
}

impl Config {
    pub fn load(path: Option<&PathBuf>) -> anyhow::Result<Self> {
        let mut cfg = if let Some(p) = path {
            let raw = std::fs::read_to_string(p)?;
            toml::from_str(&raw)?
        } else if let Ok(raw) = std::fs::read_to_string("dice-server.toml") {
            toml::from_str(&raw)?
        } else {
            Config::default()
        };

        if let Ok(v) = std::env::var("DATABASE_URL") {
            cfg.database_url = v;
        }
        if let Ok(v) = std::env::var("JWT_SECRET") {
            cfg.jwt_secret = v;
        }
        if let Ok(v) = std::env::var("DICE_STORE_PATH") {
            cfg.store_path = PathBuf::from(v);
        }
        if let Ok(v) = std::env::var("DICE_LISTEN") {
            cfg.listen = v;
        }
        if let Ok(v) = std::env::var("DICE_STORE_KEY") {
            cfg.store_key_hex = v;
        }
        if let Ok(v) = std::env::var("DICE_PRODUCTION") {
            cfg.production = matches!(v.as_str(), "1" | "true" | "TRUE" | "yes");
        }
        if let Ok(v) = std::env::var("DICE_CORS_ORIGINS") {
            cfg.cors_origins = v
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
        if let Ok(v) = std::env::var("DICE_NOTIFY_WEBHOOK") {
            cfg.notify_webhook_url = v;
        }
        if let Ok(v) = std::env::var("DICE_AGENT_DIST") {
            cfg.agent_dist_dir = PathBuf::from(v);
        }

        cfg.validate()?;
        Ok(cfg)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        let weak_jwt = self.jwt_secret.len() < 24
            || self.jwt_secret.contains("change-me")
            || self.jwt_secret.contains("dev-only")
            || self.jwt_secret == "replace-with-long-random-secret";

        if self.production {
            if weak_jwt {
                anyhow::bail!(
                    "production=true requires a strong jwt_secret (min 24 chars, not a placeholder)"
                );
            }
            if self.store_key_hex.trim().len() != 64 {
                anyhow::bail!(
                    "production=true requires store_key_hex (64 hex chars). Generate: openssl rand -hex 32"
                );
            }
            if self.cors_origins.is_empty() {
                anyhow::bail!(
                    "production=true requires cors_origins (comma-separated HTTPS origins)"
                );
            }
            if self.ldap.enabled && self.ldap.url.starts_with("ldap://") {
                tracing::warn!(
                    "LDAP URL uses cleartext ldap:// — prefer ldaps:// or StartTLS in production"
                );
            }
        } else if weak_jwt {
            tracing::warn!("jwt_secret looks weak — set a long random secret before production");
        }

        if !self.store_key_hex.trim().is_empty() && self.store_key_hex.trim().len() != 64 {
            anyhow::bail!("store_key_hex must be exactly 64 hex characters");
        }
        Ok(())
    }
}
