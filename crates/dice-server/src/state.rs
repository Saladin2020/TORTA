use std::sync::Arc;

use sqlx::PgPool;

use crate::config::Config;
use crate::hub::AgentHub;
use crate::rate_limit::LoginLimiter;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    pub hub: AgentHub,
    pub login_limiter: Arc<LoginLimiter>,
}
