//! Coral-SAAS control plane entry point.
//
// `dead_code` is allowed crate-wide while the scaffold is incomplete —
// many model methods and config fields land before their consumers do.
// Remove this attribute before launch.
#![allow(dead_code)]

use std::net::SocketAddr;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

mod audit;
mod auth;
mod config;
mod db;
mod error;
mod github_app;
mod idempotency;
mod jobs;
mod middleware;
mod r2;
mod routes;
mod state;
mod stripe;
mod wiki;

use config::Config;
use state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,api=debug")),
        )
        .with(tracing_subscriber::fmt::layer().json())
        .init();

    let config = Config::from_env()?;
    let port = config.port;

    let db = db::connect(&config.database_url).await?;

    // Run pending migrations. In production we may want to gate this
    // behind a flag and run migrations as a separate step.
    sqlx::migrate!("../migrations").run(&db).await?;

    let redis_client = redis::Client::open(config.redis_url.clone())?;
    let redis = redis::aio::ConnectionManager::new(redis_client).await?;

    let http = reqwest::Client::builder()
        .user_agent("coral-saas/0.1")
        .timeout(std::time::Duration::from_secs(15))
        .build()?;

    let state = AppState::new(config, db, redis, http);

    // Background janitor: reap stale/orphaned jobs (release repos stuck at
    // bootstrap_status='running' when a worker dies) and purge expired
    // sessions. Best-effort; a failing pass is logged and retried next tick.
    tokio::spawn(janitor(state.clone()));

    let app = routes::build_router(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!(%addr, "coral-saas api starting");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// Periodic maintenance loop. Runs every 5 minutes for the life of the
/// process; each pass is independent and best-effort.
async fn janitor(state: AppState) {
    let mut ticker = tokio::time::interval(std::time::Duration::from_secs(5 * 60));
    // The immediate first tick makes startup reap anything left stale by a
    // prior crash/redeploy.
    loop {
        ticker.tick().await;
        if let Err(e) = jobs::reap_stale_jobs(&state).await {
            tracing::error!(error = ?e, "janitor: reap_stale_jobs failed");
        }
        match db::models::Session::purge_expired(state.db()).await {
            Ok(n) if n > 0 => tracing::info!(purged = n, "janitor: expired sessions purged"),
            Ok(_) => {}
            Err(e) => tracing::error!(error = ?e, "janitor: purge_expired failed"),
        }
    }
}
