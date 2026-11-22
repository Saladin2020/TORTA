use axum::{
    extract::{Multipart, State},
    Json,
};
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::state::AppState;

pub async fn upload_files(
    user: AuthUser,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, String)> {
    let snapshot_id: (Uuid,) = sqlx::query_as(
        r#"INSERT INTO snapshots (user_id, source, label, status)
           VALUES ($1, 'web_upload', 'web upload', 'running')
           RETURNING id"#,
    )
    .bind(user.id)
    .fetch_one(&state.db)
    .await
    .map_err(internal)?;

    let mut file_count = 0i32;
    let mut raw_bytes = 0i64;
    let mut stored_bytes = 0i64;
    let chunker = dice_core::ChunkerOptions::default();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| (axum::http::StatusCode::BAD_REQUEST, e.to_string()))?
    {
        let name = field
            .file_name()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "upload.bin".into());
        let data = field
            .bytes()
            .await
            .map_err(|e| (axum::http::StatusCode::BAD_REQUEST, e.to_string()))?;

        crate::quota::assert_can_store(&state, user.id, data.len() as i64)
            .await
            .map_err(|e| (axum::http::StatusCode::INSUFFICIENT_STORAGE, e.message))?;

        let relative = format!("web/{}", name.replace('\\', "/"));
        let result = dice_core::ingest_bytes(
            &state.config.store_path,
            &data,
            &relative,
            chunker,
            1,
        )
        .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        if result.stored_bytes > 0 {
            let _ = crate::quota::bump_store_used(
                &state.db,
                &state.config.store_path.display().to_string(),
                result.stored_bytes as i64,
            )
            .await;
        }

        let manifest_json = serde_json::to_value(&result.manifest)
            .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        sqlx::query(
            r#"INSERT INTO snapshot_files (snapshot_id, relative_path, size_bytes, content_hash, manifest_json)
               VALUES ($1, $2, $3, $4, $5)"#,
        )
        .bind(snapshot_id.0)
        .bind(&result.manifest.relative_path)
        .bind(result.raw_bytes as i64)
        .bind(&result.manifest.hash)
        .bind(manifest_json)
        .execute(&state.db)
        .await
        .map_err(internal)?;

        file_count += 1;
        raw_bytes += result.raw_bytes as i64;
        stored_bytes += result.stored_bytes as i64;
    }

    sqlx::query(
        r#"UPDATE snapshots SET
             finished_at = NOW(),
             status = 'success',
             file_count = $2,
             raw_bytes = $3,
             stored_bytes = $4
           WHERE id = $1"#,
    )
    .bind(snapshot_id.0)
    .bind(file_count)
    .bind(raw_bytes)
    .bind(stored_bytes)
    .execute(&state.db)
    .await
    .map_err(internal)?;

    Ok(Json(serde_json::json!({
        "snapshot_id": snapshot_id.0,
        "file_count": file_count,
        "raw_bytes": raw_bytes,
        "stored_bytes": stored_bytes,
    })))
}

fn internal(e: sqlx::Error) -> (axum::http::StatusCode, String) {
    (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}
