use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::client::ApiClient;

/// Default queue filename when placed next to the agent config.
pub const QUEUE_FILENAME: &str = "dice-agent-queue.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueItem {
    pub last_error: String,
    pub timestamp: String,
    pub retry_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkQueue {
    #[serde(default)]
    pub items: Vec<QueueItem>,
    /// Set while a backup is in progress (cleared on success / when recording failure).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_progress_started_at: Option<String>,
}

impl WorkQueue {
    /// Resolve queue path: same directory as config (`dice-agent-queue.json`),
    /// or `./data/agent-queue.json` when config path is unknown.
    pub fn path_for_config(config_path: Option<&Path>) -> PathBuf {
        match config_path {
            Some(cfg) => cfg
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(QUEUE_FILENAME),
            None => PathBuf::from("./data/agent-queue.json"),
        }
    }

    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let raw = serde_json::to_string_pretty(self)?;
        std::fs::write(path, raw)?;
        Ok(())
    }

    pub fn record_failure(&mut self, error: &str, snapshot_id: Option<Uuid>) {
        self.in_progress_started_at = None;
        self.items.push(QueueItem {
            last_error: error.to_string(),
            timestamp: Utc::now().to_rfc3339(),
            retry_count: 0,
            snapshot_id,
        });
    }
}

/// Mark that a backup is starting (persisted for crash / offline recovery).
pub fn mark_backup_starting(queue_path: &Path) -> anyhow::Result<()> {
    let mut q = WorkQueue::load(queue_path);
    q.in_progress_started_at = Some(Utc::now().to_rfc3339());
    q.save(queue_path)?;
    Ok(())
}

/// Persist a failed attempt after retries are exhausted.
pub fn record_failure(
    queue_path: &Path,
    error: &str,
    snapshot_id: Option<Uuid>,
) -> anyhow::Result<()> {
    let mut q = WorkQueue::load(queue_path);
    q.record_failure(error, snapshot_id);
    q.save(queue_path)?;
    tracing::warn!(
        path = %queue_path.display(),
        error = %error,
        ?snapshot_id,
        "recorded failure to offline queue"
    );
    Ok(())
}

/// Clear in-progress marker after a successful backup.
pub fn clear_in_progress(queue_path: &Path) -> anyhow::Result<()> {
    let mut q = WorkQueue::load(queue_path);
    if q.in_progress_started_at.take().is_some() {
        q.save(queue_path)?;
    }
    Ok(())
}

/// Try each queued item once: finish incomplete snapshots when possible, drop on success.
pub async fn drain_once(client: &ApiClient, queue_path: &Path) -> anyhow::Result<()> {
    let mut q = WorkQueue::load(queue_path);
    if q.items.is_empty() {
        return Ok(());
    }

    tracing::info!(count = q.items.len(), "draining offline queue");
    let mut remaining = Vec::new();

    for mut item in q.items.drain(..) {
        match item.snapshot_id {
            Some(sid) => {
                match client
                    .finish_snapshot(
                        sid,
                        false,
                        Some(format!(
                            "retried from offline queue: {}",
                            item.last_error
                        )),
                        0,
                        0,
                    )
                    .await
                {
                    Ok(()) => {
                        tracing::info!(%sid, "queue item drained (snapshot finished)");
                    }
                    Err(e) => {
                        item.retry_count += 1;
                        item.last_error = e.to_string();
                        item.timestamp = Utc::now().to_rfc3339();
                        tracing::warn!(
                            %sid,
                            retry_count = item.retry_count,
                            error = %e,
                            "queue drain failed; keeping item"
                        );
                        remaining.push(item);
                    }
                }
            }
            None => {
                // No snapshot to finish — drop after one drain pass (next scheduled backup covers it).
                tracing::info!(
                    error = %item.last_error,
                    "dropping queue item without snapshot_id"
                );
            }
        }
    }

    q.items = remaining;
    q.save(queue_path)?;
    Ok(())
}
