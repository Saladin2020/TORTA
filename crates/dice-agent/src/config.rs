use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub server_url: String,
    pub agent_token: String,
    /// Local kill-switch — agent will heartbeat but skip backups when false
    pub enabled: bool,
    pub poll_interval_secs: u64,
    /// Extra paths beyond server policy (only used if server allows / merges)
    pub extra_paths: Vec<String>,
    pub config_path: Option<PathBuf>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            server_url: "http://127.0.0.1:8080".into(),
            agent_token: String::new(),
            enabled: true,
            poll_interval_secs: 60,
            extra_paths: Vec::new(),
            config_path: None,
        }
    }
}

impl AgentConfig {
    pub fn load(path: &PathBuf) -> anyhow::Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        let mut cfg: AgentConfig = toml::from_str(&raw)?;
        cfg.config_path = Some(path.clone());
        Ok(cfg)
    }

    pub fn save(&self, path: &PathBuf) -> anyhow::Result<()> {
        let mut clone = self.clone();
        clone.config_path = None;
        std::fs::write(path, toml::to_string_pretty(&clone)?)?;
        Ok(())
    }
}
