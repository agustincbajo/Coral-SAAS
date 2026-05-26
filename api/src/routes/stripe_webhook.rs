//! `/api/stripe/webhook` — verify, dedup, dispatch.
//!
//! See SAAS-PLAN.md §11.3 for the event taxonomy.

use crate::{
    audit::{self, Actor, AuditEntry},
    error::{ApiError, ApiResult},
    state::AppState,
    stripe::webhook::{verify_signature, SIGNATURE_HEADER},
};
use axum::{body::Bytes, extract::State, http::HeaderMap, response::IntoResponse, routing::post, Router};
use serde::Deserialize;
use serde_json::Value;

pub fn router() -> Router<AppState> {
    Router::new().route("/api/stripe/webhook", post(webhook))
}

#[derive(Debug, Deserialize)]
struct StripeEvent {
    id: String,
    #[serde(rename = "type")]
    event_type: String,
    data: StripeEventData,
}

#[derive(Debug, Deserialize)]
struct StripeEventData {
    object: Value,
}

async fn webhook(
    State(app): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<impl IntoResponse> {
    let signature = headers
        .get(SIGNATURE_HEADER)
        .and_then(|v| v.to_str().ok())
        .ok_or(ApiError::Forbidden)?;
    verify_signature(&app.config().stripe.webhook_secret, &body, signature)?;

    let event: StripeEvent = serde_json::from_slice(&body)?;

    // Idempotency via `stripe_events` table — strongly consistent,
    // unlike the Redis-based GitHub idempotency.
    let inserted = sqlx::query(
        "INSERT INTO stripe_events (id) VALUES ($1) ON CONFLICT (id) DO NOTHING",
    )
    .bind(&event.id)
    .execute(app.db())
    .await?
    .rows_affected();

    if inserted == 0 {
        tracing::info!(event_id = %event.id, "stripe webhook duplicate, skipping");
        return Ok(axum::http::StatusCode::OK);
    }

    tracing::info!(event_id = %event.id, event_type = %event.event_type, "stripe webhook received");

    match event.event_type.as_str() {
        "checkout.session.completed" => handle_checkout_completed(&app, &event.data.object).await?,
        "customer.subscription.created"
        | "customer.subscription.updated" => {
            handle_subscription_updated(&app, &event.data.object).await?
        }
        "customer.subscription.deleted" => {
            handle_subscription_deleted(&app, &event.data.object).await?
        }
        "invoice.payment_succeeded" => {
            // No-op for now beyond audit.
        }
        "invoice.payment_failed" => {
            // TODO: dunning flow — email user, mark account at risk.
        }
        other => {
            tracing::debug!(event_type = %other, "unhandled stripe event");
        }
    }

    let _ = audit::write(
        app.db(),
        AuditEntry {
            tenant_id: None,
            actor: Actor::WebhookStripe,
            action: &format!("stripe.{}", event.event_type),
            resource_kind: Some("stripe_event"),
            resource_id: Some(&event.id),
            metadata: None, // skip raw body — Stripe payloads include PAN-adjacent data
            legal_retention: true, // billing events
        },
    )
    .await;

    Ok(axum::http::StatusCode::OK)
}

#[derive(Debug, Deserialize)]
struct CheckoutSession {
    customer: Option<String>,
    client_reference_id: Option<String>,
}

async fn handle_checkout_completed(app: &AppState, obj: &Value) -> ApiResult<()> {
    let session: CheckoutSession = serde_json::from_value(obj.clone())?;

    let Some(tenant_id_str) = session.client_reference_id else {
        tracing::warn!("stripe checkout completed without client_reference_id");
        return Ok(());
    };
    let tenant_id: uuid::Uuid = match tenant_id_str.parse() {
        Ok(id) => id,
        Err(_) => {
            tracing::warn!(client_reference_id = %tenant_id_str, "invalid tenant_id in checkout");
            return Ok(());
        }
    };

    if let Some(customer) = session.customer {
        sqlx::query("UPDATE tenants SET stripe_customer_id = $1, plan = 'pro' WHERE id = $2")
            .bind(&customer)
            .bind(tenant_id)
            .execute(app.db())
            .await?;
    }

    Ok(())
}

#[derive(Debug, Deserialize)]
struct Subscription {
    customer: String,
    status: String,
    items: SubscriptionItems,
}

#[derive(Debug, Deserialize)]
struct SubscriptionItems {
    data: Vec<SubscriptionItem>,
}

#[derive(Debug, Deserialize)]
struct SubscriptionItem {
    price: SubscriptionPrice,
}

#[derive(Debug, Deserialize)]
struct SubscriptionPrice {
    // Plans are mapped by Stripe price lookup_key — set in product config.
    lookup_key: Option<String>,
}

async fn handle_subscription_updated(app: &AppState, obj: &Value) -> ApiResult<()> {
    let sub: Subscription = serde_json::from_value(obj.clone())?;

    let plan = if sub.status == "active" || sub.status == "trialing" {
        sub.items
            .data
            .first()
            .and_then(|i| i.price.lookup_key.as_deref())
            .unwrap_or("free")
    } else {
        "free"
    };

    sqlx::query("UPDATE tenants SET plan = $1 WHERE stripe_customer_id = $2")
        .bind(plan)
        .bind(&sub.customer)
        .execute(app.db())
        .await?;

    Ok(())
}

async fn handle_subscription_deleted(app: &AppState, obj: &Value) -> ApiResult<()> {
    let sub: Subscription = serde_json::from_value(obj.clone())?;

    sqlx::query("UPDATE tenants SET plan = 'free' WHERE stripe_customer_id = $1")
        .bind(&sub.customer)
        .execute(app.db())
        .await?;

    Ok(())
}
