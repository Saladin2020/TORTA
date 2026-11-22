use axum::{
    extract::{
        ws::{Message, WebSocket},
        Query, State, WebSocketUpgrade,
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use uuid::Uuid;

use crate::state::AppState;

#[derive(Deserialize)]
pub struct WsQuery {
    pub token: String,
}

pub async fn agent_ws(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(q): Query<WsQuery>,
) -> Result<impl IntoResponse, (axum::http::StatusCode, String)> {
    let row = sqlx::query_as::<_, (Uuid, bool)>(
        "SELECT id, enabled FROM agents WHERE agent_token = $1",
    )
    .bind(&q.token)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .ok_or((axum::http::StatusCode::UNAUTHORIZED, "unknown agent".into()))?;

    if !row.1 {
        return Err((axum::http::StatusCode::FORBIDDEN, "agent disabled".into()));
    }

    let agent_id = row.0;
    Ok(ws.on_upgrade(move |socket| handle_socket(socket, state, agent_id)))
}

async fn handle_socket(socket: WebSocket, state: AppState, agent_id: Uuid) {
    let (mut sink, mut stream) = socket.split();
    let mut rx = state.hub.subscribe(agent_id).await;

    let _ = sqlx::query(
        "UPDATE agents SET last_seen_at = NOW(), status = 'online' WHERE id = $1",
    )
    .bind(agent_id)
    .execute(&state.db)
    .await;

    let _ = sink
        .send(Message::Text(
            r#"{"type":"hello","message":"dice-agent-ws"}"#.into(),
        ))
        .await;

    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Ok(payload) => {
                        if sink.send(Message::Text(payload.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
            frame = stream.next() => {
                match frame {
                    Some(Ok(Message::Ping(p))) => {
                        let _ = sink.send(Message::Pong(p)).await;
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Text(_))) | Some(Ok(Message::Binary(_)))
                    | Some(Ok(Message::Pong(_))) => {}
                    Some(Err(_)) => break,
                }
            }
        }
    }
}
