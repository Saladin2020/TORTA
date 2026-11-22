use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

pub async fn write(
    db: &PgPool,
    actor_user_id: Option<Uuid>,
    action: &str,
    detail: Value,
) {
    if let Err(e) = sqlx::query(
        r#"INSERT INTO audit_log (actor_user_id, action, detail)
           VALUES ($1, $2, $3)"#,
    )
    .bind(actor_user_id)
    .bind(action)
    .bind(detail)
    .execute(db)
    .await
    {
        tracing::warn!(error = %e, action, "failed to write audit_log");
    }
}
