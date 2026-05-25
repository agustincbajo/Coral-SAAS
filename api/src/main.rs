//! Coral-SAAS control plane.
//!
//! Stub entry point — exposes `/healthz` so Railway's healthcheck passes.
//! Real routes (`/auth`, `/api/repos`, `/api/jobs`, `/api/github/webhook`,
//! `/api/stripe/webhook`) get wired up in Fase 1+ of the SAAS-PLAN.

use axum::{routing::get, Json, Router};
use serde::Serialize;
use std::net::SocketAddr;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[derive(Serialize)]
struct Health {
    status: &'static str,
    service: &'static str,
    version: &'static str,
}

async fn healthz() -> Json<Health> {
    Json(Health {
        status: "ok",
        service: "coral-saas-api",
        version: env!("CARGO_PKG_VERSION"),
    })
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer().json())
        .init();

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8080);

    let app = Router::new().route("/healthz", get(healthz));

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!(%addr, "coral-saas api starting");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
