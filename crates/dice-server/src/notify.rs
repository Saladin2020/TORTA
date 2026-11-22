use serde_json::json;

/// Fire-and-forget webhook notification (backup fail / quota). Best-effort.
pub async fn notify_webhook(url: &str, event: &str, detail: serde_json::Value) {
    if url.trim().is_empty() {
        return;
    }
    let body = json!({
        "service": "dice-server",
        "event": event,
        "detail": detail,
        "ts": chrono::Utc::now().to_rfc3339(),
    });
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "notify client build failed");
            return;
        }
    };
    match client.post(url).json(&body).send().await {
        Ok(resp) if resp.status().is_success() => {
            tracing::debug!(event, "notify webhook ok");
        }
        Ok(resp) => {
            tracing::warn!(event, status = %resp.status(), "notify webhook non-2xx");
        }
        Err(e) => tracing::warn!(event, error = %e, "notify webhook failed"),
    }
}
