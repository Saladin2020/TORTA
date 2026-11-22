use std::collections::HashSet;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::Local;
use walkdir::WalkDir;

use crate::client::{ApiClient, Policy};
use crate::queue;
use dice_core::{chunk_file_for_push, ChunkerOptions, ExcludeSet};

const BACKOFF_SECS: &[u64] = &[1, 2, 4];

pub async fn run_backup(
    client: &ApiClient,
    policy: &Policy,
    extra_paths: &[String],
    queue_path: Option<&Path>,
) -> anyhow::Result<()> {
    let mut paths: Vec<String> = policy.paths.clone();
    for p in extra_paths {
        if !paths.iter().any(|x| x == p) {
            paths.push(p.clone());
        }
    }

    if paths.is_empty() {
        tracing::warn!("no paths configured in policy");
        return Ok(());
    }

    let excludes = if policy.excludes.is_empty() {
        ExcludeSet::from_defaults()
    } else {
        ExcludeSet::from_list(policy.excludes.clone())
    };

    let label = format!("agent {}", Local::now().format("%Y-%m-%d %H:%M"));
    let snapshot_id = match with_retry("begin_snapshot", || client.begin_snapshot(&label)).await {
        Ok(id) => id,
        Err(e) => {
            if let Some(qp) = queue_path {
                let _ = queue::record_failure(qp, &e.to_string(), None);
            }
            return Err(e);
        }
    };
    tracing::info!(%snapshot_id, "snapshot started");

    let chunker = ChunkerOptions::default();
    let mut raw_bytes = 0u64;
    let mut stored_bytes = 0u64;
    let mut errors: Vec<String> = Vec::new();
    let bandwidth_bps = policy.bandwidth_bps.filter(|&b| b > 0);

    for path_spec in &paths {
        let root = resolve_user_path(path_spec);
        if !root.exists() {
            tracing::warn!(path = %root.display(), "path missing, skip");
            continue;
        }

        for file in collect_files(&root, &excludes) {
            let rel = match relative_label(&root, path_spec, &file) {
                Ok(r) => r,
                Err(e) => {
                    errors.push(e.to_string());
                    continue;
                }
            };

            match push_file(
                client,
                snapshot_id,
                &file,
                &rel,
                chunker,
                bandwidth_bps,
            )
            .await
            {
                Ok((raw, stored)) => {
                    raw_bytes += raw;
                    stored_bytes += stored;
                    tracing::info!(%rel, raw, stored, "backed up");
                }
                Err(e) => {
                    tracing::error!(%rel, error = %e, "file failed");
                    if is_transient(&e) {
                        if let Some(qp) = queue_path {
                            let _ = queue::record_failure(qp, &e.to_string(), Some(snapshot_id));
                        }
                        return Err(e);
                    }
                    errors.push(format!("{rel}: {e}"));
                }
            }
        }
    }

    let success = errors.is_empty();
    if let Err(e) = with_retry("finish_snapshot", || {
        client.finish_snapshot(
            snapshot_id,
            success,
            if success {
                None
            } else {
                Some(errors.join("; "))
            },
            raw_bytes,
            stored_bytes,
        )
    })
    .await
    {
        if let Some(qp) = queue_path {
            let _ = queue::record_failure(qp, &e.to_string(), Some(snapshot_id));
        }
        return Err(e);
    }

    if let Some(qp) = queue_path {
        let _ = queue::clear_in_progress(qp);
    }

    if success {
        tracing::info!(raw_bytes, stored_bytes, "backup finished OK");
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "backup finished with errors: {}",
            errors.join("; ")
        ))
    }
}

async fn push_file(
    client: &ApiClient,
    snapshot_id: uuid::Uuid,
    file: &Path,
    relative: &str,
    chunker: ChunkerOptions,
    bandwidth_bps: Option<i64>,
) -> anyhow::Result<(u64, u64)> {
    let (manifest, payloads) = chunk_file_for_push(file, relative, chunker, 1)?;
    let hashes: Vec<String> = payloads.iter().map(|(h, _)| h.clone()).collect();
    let missing: HashSet<String> = with_retry("missing_chunks", || client.missing_chunks(&hashes))
        .await?
        .into_iter()
        .collect();

    let mut stored = 0u64;
    for (hash, data) in &payloads {
        if missing.contains(hash) {
            with_retry("upload_chunk", || client.upload_chunk(hash, data)).await?;
            stored += data.len() as u64;
            throttle_after_chunk(data.len(), bandwidth_bps).await;
        }
    }

    with_retry("commit_file", || {
        client.commit_file(
            snapshot_id,
            relative,
            manifest.size,
            &manifest.hash,
            &manifest,
        )
    })
    .await?;

    Ok((manifest.size, stored))
}

async fn throttle_after_chunk(bytes: usize, bandwidth_bps: Option<i64>) {
    let Some(bps) = bandwidth_bps else {
        return;
    };
    if bps <= 0 || bytes == 0 {
        return;
    }
    let secs = bytes as f64 / bps as f64;
    if secs > 0.0 {
        tokio::time::sleep(Duration::from_secs_f64(secs)).await;
    }
}

async fn with_retry<F, Fut, T>(op: &str, mut f: F) -> anyhow::Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = anyhow::Result<T>>,
{
    let mut attempt = 0usize;
    loop {
        match f().await {
            Ok(v) => return Ok(v),
            Err(e) => {
                if is_transient(&e) && attempt < BACKOFF_SECS.len() {
                    let delay = BACKOFF_SECS[attempt];
                    attempt += 1;
                    tracing::warn!(
                        op,
                        attempt,
                        delay_secs = delay,
                        error = %e,
                        "transient failure, retrying"
                    );
                    tokio::time::sleep(Duration::from_secs(delay)).await;
                } else {
                    return Err(e);
                }
            }
        }
    }
}

fn is_transient(err: &anyhow::Error) -> bool {
    for cause in err.chain() {
        if let Some(re) = cause.downcast_ref::<reqwest::Error>() {
            if re.is_timeout() || re.is_connect() || re.is_request() {
                return true;
            }
            if let Some(status) = re.status() {
                if status.is_server_error() || status.as_u16() == 429 {
                    return true;
                }
            }
        }
    }
    let s = err.to_string().to_lowercase();
    s.contains("timeout")
        || s.contains("connection")
        || s.contains("temporarily")
        || s.contains("broken pipe")
        || s.contains("reset")
}

fn collect_files(root: &Path, excludes: &ExcludeSet) -> Vec<PathBuf> {
    if root.is_file() {
        return vec![root.to_path_buf()];
    }
    let mut out = Vec::new();
    for entry in WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                if excludes.matches_name(name) {
                    continue;
                }
            }
        }
        if excludes.matches_path(path) {
            continue;
        }
        if path.is_file() {
            out.push(path.to_path_buf());
        }
    }
    out
}

fn relative_label(root: &Path, path_spec: &str, file: &Path) -> anyhow::Result<String> {
    if root.is_file() {
        let name = root
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".into());
        return Ok(format!("{path_spec}/{name}").replace('\\', "/"));
    }
    let rel = file.strip_prefix(root)?;
    let joined = format!(
        "{}/{}",
        path_spec.trim_matches(['/', '\\']),
        rel.display()
    );
    Ok(joined.replace('\\', "/"))
}

fn resolve_user_path(spec: &str) -> PathBuf {
    let home = dirs_home();
    match spec {
        "Documents" => home.join("Documents"),
        "Desktop" => home.join("Desktop"),
        "Downloads" => home.join("Downloads"),
        other if Path::new(other).is_absolute() => PathBuf::from(other),
        other => home.join(other),
    }
}

fn dirs_home() -> PathBuf {
    if let Ok(h) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
        return PathBuf::from(h);
    }
    PathBuf::from(".")
}
