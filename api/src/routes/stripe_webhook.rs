//! `/api/stripe/webhook` — stub. Filled in by Fase 2 (stripe/).

use crate::state::AppState;
use axum::{http::StatusCode, routing::post, Router};

async fn webhook() -> StatusCode {
    // TODO Fase 2: signature validation, idempotency via `stripe_events`
    // table, subscription state sync. See SAAS-PLAN.md §11.3.
    StatusCode::NOT_IMPLEMENTED
}

pub fn router() -> Router<AppState> {
    Router::new().route("/api/stripe/webhook", post(webhook))
}
