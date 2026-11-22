use std::io::{Cursor, Write};

use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use crate::audit;
use crate::auth::AuthUser;
use crate::state::AppState;

#[derive(Serialize, sqlx::FromRow)]
pub struct SnapshotRow {
    pub id: Uuid,
    pub label: String,
    pub source: String,
    pub status: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub file_count: i32,
    pub raw_bytes: i64,
    pub stored_bytes: i64,
    pub deleted_at: Option<DateTime<Utc>>,
}

pub async fn list_snapshots(
    user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<SnapshotRow>>, (axum::http::StatusCode, String)> {
    let rows = sqlx::query_as::<_, SnapshotRow>(
        r#"SELECT id, label, source::text, status::text, started_at, finished_at,
                  file_count, raw_bytes, stored_bytes, deleted_at
           FROM snapshots
           WHERE user_id = $1 AND deleted_at IS NULL
           ORDER BY started_at DESC
           LIMIT 200"#,
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await
    .map_err(internal)?;
    Ok(Json(rows))
}

#[derive(Serialize)]
pub struct ChunkInfo {
    pub index: usize,
    pub hash: String,
    pub raw_size: usize,
    pub compressed_size: usize,
}

#[derive(Serialize)]
pub struct FileRow {
    pub id: Uuid,
    pub snapshot_id: Uuid,
    pub relative_path: String,
    pub size_bytes: i64,
    pub content_hash: String,
    pub chunks: Vec<ChunkInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backed_up_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
}

fn chunks_from_manifest(manifest: &serde_json::Value) -> Vec<ChunkInfo> {
    manifest
        .get("chunks")
        .and_then(|c| c.as_array())
        .map(|arr| {
            arr.iter()
                .enumerate()
                .filter_map(|(index, c)| {
                    Some(ChunkInfo {
                        index,
                        hash: c.get("hash")?.as_str()?.to_string(),
                        raw_size: c.get("rawSize").or_else(|| c.get("raw_size"))?.as_u64()?
                            as usize,
                        compressed_size: c
                            .get("compressedSize")
                            .or_else(|| c.get("compressed_size"))?
                            .as_u64()? as usize,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Latest file per logical path (for download UX).
pub async fn list_path_files(
    user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<FileRow>>, (axum::http::StatusCode, String)> {
    let rows = sqlx::query_as::<
        _,
        (
            Uuid,
            Uuid,
            String,
            i64,
            String,
            serde_json::Value,
            DateTime<Utc>,
        ),
    >(
        r#"SELECT DISTINCT ON (f.relative_path)
                f.id, f.snapshot_id, f.relative_path, f.size_bytes, f.content_hash,
                f.manifest_json, s.started_at
           FROM snapshot_files f
           JOIN snapshots s ON s.id = f.snapshot_id
           WHERE s.user_id = $1 AND s.status = 'success' AND s.deleted_at IS NULL
           ORDER BY f.relative_path ASC, s.started_at DESC"#,
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await
    .map_err(internal)?;

    let out = rows
        .into_iter()
        .map(
            |(id, snapshot_id, relative_path, size_bytes, content_hash, manifest, backed_up_at)| {
                FileRow {
                    id,
                    snapshot_id,
                    relative_path,
                    size_bytes,
                    content_hash,
                    chunks: chunks_from_manifest(&manifest),
                    backed_up_at: Some(backed_up_at),
                    aliases: Vec::new(),
                }
            },
        )
        .collect();
    Ok(Json(out))
}

/// Dice store view: one row per unique content hash (matches on-disk dedup).
pub async fn list_unique_store(
    user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<FileRow>>, (axum::http::StatusCode, String)> {
    let rows = sqlx::query_as::<
        _,
        (
            Uuid,
            Uuid,
            String,
            i64,
            String,
            serde_json::Value,
            DateTime<Utc>,
            Vec<String>,
        ),
    >(
        r#"
        WITH latest AS (
          SELECT DISTINCT ON (f.content_hash)
                 f.id, f.snapshot_id, f.relative_path, f.size_bytes, f.content_hash,
                 f.manifest_json, s.started_at
          FROM snapshot_files f
          JOIN snapshots s ON s.id = f.snapshot_id
          WHERE s.user_id = $1 AND s.status = 'success' AND s.deleted_at IS NULL
          ORDER BY f.content_hash, s.started_at DESC
        ),
        aliases AS (
          SELECT f.content_hash,
                 array_agg(DISTINCT f.relative_path ORDER BY f.relative_path) AS paths
          FROM snapshot_files f
          JOIN snapshots s ON s.id = f.snapshot_id
          WHERE s.user_id = $1 AND s.status = 'success' AND s.deleted_at IS NULL
          GROUP BY f.content_hash
        )
        SELECT l.id, l.snapshot_id, l.relative_path, l.size_bytes, l.content_hash,
               l.manifest_json, l.started_at, a.paths
        FROM latest l
        JOIN aliases a ON a.content_hash = l.content_hash
        ORDER BY l.relative_path
        "#,
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await
    .map_err(internal)?;

    let out = rows
        .into_iter()
        .map(
            |(
                id,
                snapshot_id,
                relative_path,
                size_bytes,
                content_hash,
                manifest,
                backed_up_at,
                paths,
            )| {
                let aliases: Vec<String> = paths
                    .into_iter()
                    .filter(|p| p != &relative_path)
                    .collect();
                FileRow {
                    id,
                    snapshot_id,
                    relative_path,
                    size_bytes,
                    content_hash,
                    chunks: chunks_from_manifest(&manifest),
                    backed_up_at: Some(backed_up_at),
                    aliases,
                }
            },
        )
        .collect();
    Ok(Json(out))
}

pub async fn list_files(
    user: AuthUser,
    State(state): State<AppState>,
    Path(snapshot_id): Path<Uuid>,
) -> Result<Json<Vec<FileRow>>, (axum::http::StatusCode, String)> {
    let owned: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM snapshots WHERE id = $1 AND user_id = $2 AND deleted_at IS NULL",
    )
    .bind(snapshot_id)
    .bind(user.id)
    .fetch_optional(&state.db)
    .await
    .map_err(internal)?;
    if owned.is_none() && !user.is_admin() {
        return Err((axum::http::StatusCode::NOT_FOUND, "not found".into()));
    }

    let rows = sqlx::query_as::<_, (Uuid, String, i64, String, serde_json::Value)>(
        r#"SELECT id, relative_path, size_bytes, content_hash, manifest_json
           FROM snapshot_files WHERE snapshot_id = $1
           ORDER BY relative_path"#,
    )
    .bind(snapshot_id)
    .fetch_all(&state.db)
    .await
    .map_err(internal)?;

    let out = rows
        .into_iter()
        .map(|(id, relative_path, size_bytes, content_hash, manifest)| FileRow {
            id,
            snapshot_id,
            relative_path,
            size_bytes,
            content_hash,
            chunks: chunks_from_manifest(&manifest),
            backed_up_at: None,
            aliases: Vec::new(),
        })
        .collect();

    Ok(Json(out))
}

async fn load_owned_file(
    state: &AppState,
    user: &AuthUser,
    snapshot_id: Uuid,
    file_id: Uuid,
) -> Result<(String, serde_json::Value), (axum::http::StatusCode, String)> {
    let row = sqlx::query_as::<_, (String, serde_json::Value, Uuid)>(
        r#"SELECT f.relative_path, f.manifest_json, s.user_id
           FROM snapshot_files f
           JOIN snapshots s ON s.id = f.snapshot_id
           WHERE f.id = $1 AND f.snapshot_id = $2 AND s.deleted_at IS NULL"#,
    )
    .bind(file_id)
    .bind(snapshot_id)
    .fetch_optional(&state.db)
    .await
    .map_err(internal)?
    .ok_or((axum::http::StatusCode::NOT_FOUND, "file not found".into()))?;

    if row.2 != user.id && !user.is_admin() {
        return Err((axum::http::StatusCode::FORBIDDEN, "forbidden".into()));
    }
    Ok((row.0, row.1))
}

pub async fn download_file(
    user: AuthUser,
    State(state): State<AppState>,
    Path((snapshot_id, file_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, (axum::http::StatusCode, String)> {
    let (path, manifest_json) = load_owned_file(&state, &user, snapshot_id, file_id).await?;
    let manifest: dice_core::Manifest = serde_json::from_value(manifest_json)
        .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let bytes = dice_core::assemble_bytes(&state.config.store_path, &manifest)
        .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    audit::write(
        &state.db,
        Some(user.id),
        "portal.download",
        serde_json::json!({ "snapshot_id": snapshot_id, "file_id": file_id, "path": path }),
    )
    .await;

    let filename = path.rsplit('/').next().unwrap_or("download.bin");
    let mut response = bytes.into_response();
    let headers = response.headers_mut();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/octet-stream"),
    );
    headers.insert(
        axum::http::header::CONTENT_DISPOSITION,
        axum::http::HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            .unwrap_or_else(|_| axum::http::HeaderValue::from_static("attachment")),
    );
    Ok(response)
}

/// Reassemble + hash-verify a single file from the store.
pub async fn verify_file(
    user: AuthUser,
    State(state): State<AppState>,
    Path((snapshot_id, file_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, String)> {
    let (path, manifest_json) = load_owned_file(&state, &user, snapshot_id, file_id).await?;
    let manifest: dice_core::Manifest = serde_json::from_value(manifest_json)
        .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let store = state.config.store_path.clone();
    let result = tokio::task::spawn_blocking(move || dice_core::assemble_bytes(&store, &manifest))
        .await
        .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    match result {
        Ok(bytes) => {
            audit::write(
                &state.db,
                Some(user.id),
                "portal.verify_ok",
                serde_json::json!({ "snapshot_id": snapshot_id, "file_id": file_id, "path": path }),
            )
            .await;
            Ok(Json(serde_json::json!({
                "ok": true,
                "path": path,
                "bytes": bytes.len(),
                "hash": blake3::hash(&bytes).to_hex().to_string(),
            })))
        }
        Err(e) => {
            audit::write(
                &state.db,
                Some(user.id),
                "portal.verify_fail",
                serde_json::json!({
                    "snapshot_id": snapshot_id,
                    "file_id": file_id,
                    "path": path,
                    "error": e.to_string()
                }),
            )
            .await;
            Err((
                axum::http::StatusCode::UNPROCESSABLE_ENTITY,
                format!("verify failed: {e}"),
            ))
        }
    }
}

/// Download entire snapshot as ZIP (folder restore).
pub async fn download_snapshot_zip(
    user: AuthUser,
    State(state): State<AppState>,
    Path(snapshot_id): Path<Uuid>,
) -> Result<Response, (axum::http::StatusCode, String)> {
    let owned: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM snapshots WHERE id = $1 AND (user_id = $2 OR $3) AND deleted_at IS NULL",
    )
    .bind(snapshot_id)
    .bind(user.id)
    .bind(user.is_admin())
    .fetch_optional(&state.db)
    .await
    .map_err(internal)?;
    if owned.is_none() {
        return Err((axum::http::StatusCode::NOT_FOUND, "not found".into()));
    }

    let rows = sqlx::query_as::<_, (String, serde_json::Value)>(
        "SELECT relative_path, manifest_json FROM snapshot_files WHERE snapshot_id = $1 ORDER BY relative_path",
    )
    .bind(snapshot_id)
    .fetch_all(&state.db)
    .await
    .map_err(internal)?;

    let store = state.config.store_path.clone();
    let zip_bytes = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<u8>> {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut buf);
            let opts = SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            for (rel, manifest_json) in rows {
                let manifest: dice_core::Manifest = serde_json::from_value(manifest_json)?;
                let data = dice_core::assemble_bytes(&store, &manifest)?;
                let name = rel.trim_start_matches('/').replace('\\', "/");
                zip.start_file(name, opts)?;
                zip.write_all(&data)?;
            }
            zip.finish()?;
        }
        Ok(buf.into_inner())
    })
    .await
    .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    audit::write(
        &state.db,
        Some(user.id),
        "portal.zip_download",
        serde_json::json!({ "snapshot_id": snapshot_id, "bytes": zip_bytes.len() }),
    )
    .await;

    let mut response = zip_bytes.into_response();
    let headers = response.headers_mut();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/zip"),
    );
    headers.insert(
        axum::http::header::CONTENT_DISPOSITION,
        axum::http::HeaderValue::from_str(&format!(
            "attachment; filename=\"snapshot-{snapshot_id}.zip\""
        ))
        .unwrap_or_else(|_| axum::http::HeaderValue::from_static("attachment")),
    );
    Ok(response)
}

/// Soft-delete a snapshot (trash). Hard-deleted by retention after 7 days.
pub async fn soft_delete_snapshot(
    user: AuthUser,
    State(state): State<AppState>,
    Path(snapshot_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, String)> {
    let updated = sqlx::query(
        r#"UPDATE snapshots SET deleted_at = NOW()
           WHERE id = $1 AND user_id = $2 AND deleted_at IS NULL"#,
    )
    .bind(snapshot_id)
    .bind(user.id)
    .execute(&state.db)
    .await
    .map_err(internal)?
    .rows_affected();
    if updated == 0 {
        return Err((axum::http::StatusCode::NOT_FOUND, "not found".into()));
    }
    audit::write(
        &state.db,
        Some(user.id),
        "portal.snapshot_trash",
        serde_json::json!({ "snapshot_id": snapshot_id }),
    )
    .await;
    Ok(Json(serde_json::json!({ "ok": true, "deleted_at": "now" })))
}

fn internal(e: sqlx::Error) -> (axum::http::StatusCode, String) {
    (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}
