//! HTTP routes — split by resource. Each submodule exposes one or two
//! `Router::new().route(...)` chains; `build_router()` assembles them.

use crate::{middleware::request_id, state::AppState};
use axum::{middleware, Router};
use tower_cookies::CookieManagerLayer;
use tower_http::trace::TraceLayer;

pub mod auth;
pub mod github_install;
pub mod github_webhook;
pub mod health;
pub mod jobs;
pub mod me;
pub mod repos;
pub mod stripe_webhook;

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .merge(health::router())
        .merge(auth::router())
        .merge(me::router())
        .merge(repos::router())
        .merge(jobs::router())
        .merge(github_install::router())
        .merge(github_webhook::router())
        .merge(stripe_webhook::router())
        .layer(middleware::from_fn(request_id::middleware))
        .layer(CookieManagerLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
