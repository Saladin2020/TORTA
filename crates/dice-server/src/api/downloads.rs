use std::path::{Path, PathBuf};

use axum::{
    body::Body,
    extract::{Path as AxumPath, State},
    http::{header, StatusCode},
    response::Response,
    Json,
};
use serde::Serialize;

use crate::auth::AuthUser;
use crate::state::AppState;

#[derive(Serialize)]
pub struct AgentDownload {
    pub id: String,
    pub label: String,
    pub filename: String,
    pub available: bool,
    pub size_bytes: Option<u64>,
}

struct Artifact {
    id: &'static str,
    label: &'static str,
    filename: &'static str,
    /// Relative candidates under agent_dist_dir, then optional workspace fallbacks.
    dist_names: &'static [&'static str],
    fallbacks: &'static [&'static str],
}

const ARTIFACTS: &[Artifact] = &[
    Artifact {
        id: "windows-x64",
        label: "Windows x64 (dice-agent.exe)",
        filename: "dice-agent-windows-x64.exe",
        dist_names: &["dice-agent-windows-x64.exe", "dice-agent.exe"],
        fallbacks: &["target/release/dice-agent.exe", "target/debug/dice-agent.exe"],
    },
    Artifact {
        id: "linux-x64",
        label: "Linux x64 (dice-agent)",
        filename: "dice-agent-linux-x64",
        dist_names: &["dice-agent-linux-x64", "dice-agent"],
        fallbacks: &["target/release/dice-agent", "target/debug/dice-agent"],
    },
    Artifact {
        id: "install-windows",
        label: "สคริปต์ติดตั้ง Windows (.ps1)",
        filename: "install-agent.ps1",
        dist_names: &["install-agent.ps1"],
        fallbacks: &["deploy/install-agent.ps1"],
    },
    Artifact {
        id: "install-linux",
        label: "สคริปต์ติดตั้ง Linux (.sh)",
        filename: "install-agent.sh",
        dist_names: &["install-agent.sh"],
        fallbacks: &["deploy/install-agent.sh"],
    },
];

fn resolve_artifact(dist: &Path, art: &Artifact) -> Option<PathBuf> {
    for name in art.dist_names {
        let p = dist.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    for rel in art.fallbacks {
        let p = PathBuf::from(rel);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

pub async fn list_agent_downloads(
    _user: AuthUser,
    State(state): State<AppState>,
) -> Json<Vec<AgentDownload>> {
    let dist = &state.config.agent_dist_dir;
    let list = ARTIFACTS
        .iter()
        .map(|art| {
            let path = resolve_artifact(dist, art);
            let size = path
                .as_ref()
                .and_then(|p| std::fs::metadata(p).ok())
                .map(|m| m.len());
            AgentDownload {
                id: art.id.into(),
                label: art.label.into(),
                filename: art.filename.into(),
                available: path.is_some(),
                size_bytes: size,
            }
        })
        .collect();
    Json(list)
}

pub async fn download_agent(
    user: AuthUser,
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Response, (StatusCode, String)> {
    let art = ARTIFACTS
        .iter()
        .find(|a| a.id == id)
        .ok_or((StatusCode::NOT_FOUND, "unknown agent package".into()))?;

    let path = resolve_artifact(&state.config.agent_dist_dir, art).ok_or((
        StatusCode::NOT_FOUND,
        format!(
            "artifact '{id}' not published — run deploy/publish-agent and set agent_dist_dir"
        ),
    ))?;

    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    crate::audit::write(
        &state.db,
        Some(user.id),
        "portal.agent_download",
        serde_json::json!({ "id": id, "filename": art.filename }),
    )
    .await;

    let mut res = Response::new(Body::from(bytes));
    let headers = res.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/octet-stream"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        header::HeaderValue::from_str(&format!("attachment; filename=\"{}\"", art.filename))
            .unwrap_or_else(|_| header::HeaderValue::from_static("attachment")),
    );
    Ok(res)
}
