//! `/api/github/webhook` — stub. Filled in by Fase 2 (github_app/).

use crate::state::AppState;
use axum::{http::StatusCode, routing::post, Router};

async fn webhook() -> StatusCode {
    // TODO Fase 2: HMAC validation, idempotency via Redis, dispatch by
    // event type, enqueue jobs. See SAAS-PLAN.md §7.3 + §7.4.
    StatusCode::NOT_IMPLEMENTED
}

pub fn router() -> Router<AppState> {
    Router::new().route("/api/github/webhook", post(webhook))
}
