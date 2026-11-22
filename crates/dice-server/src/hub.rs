use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

#[derive(Clone)]
pub struct AgentHub {
    inner: Arc<RwLock<HashMap<Uuid, broadcast::Sender<String>>>>,
}

impl AgentHub {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn subscribe(&self, agent_id: Uuid) -> broadcast::Receiver<String> {
        let mut map = self.inner.write().await;
        if let Some(tx) = map.get(&agent_id) {
            return tx.subscribe();
        }
        let (tx, rx) = broadcast::channel(16);
        map.insert(agent_id, tx);
        rx
    }

    pub async fn notify(&self, agent_id: Uuid, message: impl Into<String>) {
        let map = self.inner.read().await;
        if let Some(tx) = map.get(&agent_id) {
            let _ = tx.send(message.into());
        }
    }
}

impl Default for AgentHub {
    fn default() -> Self {
        Self::new()
    }
}
