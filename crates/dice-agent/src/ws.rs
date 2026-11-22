use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::config::AgentConfig;

#[derive(Debug, Clone)]
pub enum PushCmd {
    RunNow,
}

fn ws_url(server_url: &str) -> String {
    let base = server_url.trim_end_matches('/');
    if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{rest}/api/agent/ws")
    } else if let Some(rest) = base.strip_prefix("http://") {
        format!("ws://{rest}/api/agent/ws")
    } else {
        format!("ws://{base}/api/agent/ws")
    }
}

/// Persistent reconnecting websocket; pushes RunNow into `tx`.
pub async fn run_push_loop(cfg: AgentConfig, tx: mpsc::Sender<PushCmd>) {
    let url = format!("{}?token={}", ws_url(&cfg.server_url), cfg.agent_token);
    loop {
        match connect_async(&url).await {
            Ok((ws, _)) => {
                tracing::info!("agent websocket connected");
                let (mut sink, mut stream) = ws.split();
                // keepalive ping task
                let mut ping = tokio::time::interval(Duration::from_secs(25));
                loop {
                    tokio::select! {
                        _ = ping.tick() => {
                            if sink.send(Message::Ping(vec![1,2,3].into())).await.is_err() {
                                break;
                            }
                        }
                        frame = stream.next() => {
                            match frame {
                                Some(Ok(Message::Text(text))) => {
                                    if text.contains("run_now") {
                                        let _ = tx.send(PushCmd::RunNow).await;
                                    }
                                }
                                Some(Ok(Message::Ping(p))) => {
                                    let _ = sink.send(Message::Pong(p)).await;
                                }
                                Some(Ok(Message::Close(_))) | None => break,
                                Some(Err(e)) => {
                                    tracing::warn!(error = %e, "ws read error");
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                }
                tracing::warn!("agent websocket disconnected");
            }
            Err(e) => {
                tracing::warn!(error = %e, "ws connect failed");
            }
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}
