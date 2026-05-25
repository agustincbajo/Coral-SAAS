//! Coral-SAAS worker — Redis queue consumer + `coral` subprocess runner.
//!
//! Stub entry point — connects to Redis and idles. Real logic (pull job
//! → spawn `coral` child → upload result → report) gets wired up in
//! Fase 3 of the SAAS-PLAN.

use std::time::Duration;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer().json())
        .init();

    let redis_url =
        std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string());

    tracing::info!(redis = %mask_password(&redis_url), "coral-saas worker starting");

    // Smoke-test the Redis connection so an unhealthy worker exits fast
    // and Railway recycles it instead of silently idling.
    let client = redis::Client::open(redis_url.clone())?;
    let mut conn = client.get_multiplexed_async_connection().await?;
    let pong: String = redis::cmd("PING").query_async(&mut conn).await?;
    tracing::info!(?pong, "redis ready");

    // Stub loop — replace with `BLPOP coral:jobs` once the queue protocol is defined.
    loop {
        tokio::time::sleep(Duration::from_secs(30)).await;
        tracing::debug!("worker heartbeat");
    }
}

/// Strip the password from `redis://user:pass@host:port` for logging.
fn mask_password(url: &str) -> String {
    if let Some(at) = url.find('@') {
        if let Some(scheme_end) = url.find("://") {
            let after_scheme = scheme_end + 3;
            if at > after_scheme {
                return format!("{}***@{}", &url[..after_scheme], &url[at + 1..]);
            }
        }
    }
    url.to_string()
}
