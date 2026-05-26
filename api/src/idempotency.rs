//! Idempotency helper backed by Redis.
//!
//! GitHub and Stripe both can re-deliver webhooks. We tag each delivery
//! with its event id and drop duplicates via `SET key value NX EX ttl`.

use crate::error::ApiResult;
use redis::AsyncCommands;
use std::time::Duration;

const DEFAULT_TTL: Duration = Duration::from_secs(60 * 60 * 24);

/// Returns `true` if this is the first time we've seen this event id.
/// Returns `false` if a previous call already processed it — caller
/// should short-circuit with 200 OK.
pub async fn first_seen(
    redis: &mut redis::aio::ConnectionManager,
    namespace: &str,
    event_id: &str,
    ttl: Option<Duration>,
) -> ApiResult<bool> {
    let key = format!("idempotency:{}:{}", namespace, event_id);
    let ttl = ttl.unwrap_or(DEFAULT_TTL);

    let acquired: bool = redis
        .set_options(
            &key,
            "1",
            redis::SetOptions::default()
                .conditional_set(redis::ExistenceCheck::NX)
                .with_expiration(redis::SetExpiry::EX(ttl.as_secs())),
        )
        .await?;

    Ok(acquired)
}
