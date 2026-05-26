//! Shared application state injected into every route handler.

use crate::config::Config;
use redis::aio::ConnectionManager;
use sqlx::PgPool;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState(Arc<AppStateInner>);

pub struct AppStateInner {
    pub config: Config,
    pub db: PgPool,
    pub redis: ConnectionManager,
    pub http: reqwest::Client,
}

impl AppState {
    pub fn new(
        config: Config,
        db: PgPool,
        redis: ConnectionManager,
        http: reqwest::Client,
    ) -> Self {
        Self(Arc::new(AppStateInner {
            config,
            db,
            redis,
            http,
        }))
    }

    pub fn config(&self) -> &Config {
        &self.0.config
    }

    pub fn db(&self) -> &PgPool {
        &self.0.db
    }

    pub fn redis(&self) -> ConnectionManager {
        self.0.redis.clone()
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.0.http
    }
}
