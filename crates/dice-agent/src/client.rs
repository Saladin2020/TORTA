use reqwest::Client;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::AgentConfig;

#[derive(Clone)]
pub struct ApiClient {
    http: Client,
    base: String,
    token: String,
}

#[derive(Debug, Deserialize)]
pub struct Policy {
    pub enabled: bool,
    pub local_enabled: bool,
    pub run_now: bool,
    pub schedule_cron: String,
    pub paths: Vec<String>,
    pub excludes: Vec<String>,
    #[allow(dead_code)]
    pub retention_days: i32,
    pub bandwidth_bps: Option<i64>,
}

impl ApiClient {
    pub fn new(cfg: &AgentConfig) -> Self {
        Self {
            http: Client::new(),
            base: cfg.server_url.trim_end_matches('/').to_string(),
            token: cfg.agent_token.clone(),
        }
    }

    pub fn reconfigure(&mut self, cfg: &AgentConfig) {
        self.base = cfg.server_url.trim_end_matches('/').to_string();
        self.token = cfg.agent_token.clone();
    }

    fn agent_req(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        builder.header("X-Agent-Token", &self.token)
    }

    pub async fn heartbeat(
        &self,
        hostname: &str,
        os: &str,
        local_enabled: bool,
    ) -> anyhow::Result<()> {
        let body = serde_json::json!({
            "hostname": hostname,
            "os": os,
            "local_enabled": local_enabled,
        });
        self.agent_req(self.http.post(format!("{}/api/agent/heartbeat", self.base)))
            .json(&body)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub async fn get_policy(&self) -> anyhow::Result<Policy> {
        let resp = self
            .agent_req(self.http.get(format!("{}/api/agent/policy", self.base)))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(resp)
    }

    pub async fn missing_chunks(&self, hashes: &[String]) -> anyhow::Result<Vec<String>> {
        #[derive(Serialize)]
        struct Body<'a> {
            hashes: &'a [String],
        }
        #[derive(Deserialize)]
        struct Resp {
            missing: Vec<String>,
        }
        let resp: Resp = self
            .agent_req(self.http.post(format!("{}/api/agent/chunks/missing", self.base)))
            .json(&Body { hashes })
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(resp.missing)
    }

    pub async fn upload_chunk(&self, hash: &str, data: &[u8]) -> anyhow::Result<()> {
        self.agent_req(self.http.post(format!("{}/api/agent/chunks", self.base)))
            .header("X-Chunk-Hash", hash)
            .header("Content-Type", "application/octet-stream")
            .body(data.to_vec())
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub async fn begin_snapshot(&self, label: &str) -> anyhow::Result<Uuid> {
        #[derive(Deserialize)]
        struct Resp {
            snapshot_id: Uuid,
        }
        let resp: Resp = self
            .agent_req(self.http.post(format!("{}/api/agent/snapshots", self.base)))
            .json(&serde_json::json!({ "label": label }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(resp.snapshot_id)
    }

    pub async fn commit_file(
        &self,
        snapshot_id: Uuid,
        relative_path: &str,
        size_bytes: u64,
        content_hash: &str,
        manifest: &dice_core::Manifest,
    ) -> anyhow::Result<()> {
        self.agent_req(
            self.http
                .post(format!("{}/api/agent/snapshots/{snapshot_id}/files", self.base)),
        )
        .json(&serde_json::json!({
            "relative_path": relative_path,
            "size_bytes": size_bytes,
            "content_hash": content_hash,
            "manifest": manifest,
        }))
        .send()
        .await?
        .error_for_status()?;
        Ok(())
    }

    pub async fn finish_snapshot(
        &self,
        snapshot_id: Uuid,
        success: bool,
        error_message: Option<String>,
        raw_bytes: u64,
        stored_bytes: u64,
    ) -> anyhow::Result<()> {
        self.agent_req(
            self.http
                .post(format!("{}/api/agent/snapshots/{snapshot_id}/finish", self.base)),
        )
        .json(&serde_json::json!({
            "success": success,
            "error_message": error_message,
            "raw_bytes": raw_bytes,
            "stored_bytes": stored_bytes,
        }))
        .send()
        .await?
        .error_for_status()?;
        Ok(())
    }
}
