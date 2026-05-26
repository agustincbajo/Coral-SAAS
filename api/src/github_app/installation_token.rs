//! Cache installation tokens in Redis with a single-flight refresh.
//!
//! Tokens live 1h on GitHub's side. We cache for 50min and refresh
//! single-flight via `SET key NX EX 30` lock (GAP #2 in
//! SAAS-PLAN-GAPS.md) so concurrent workers don't thunder GitHub.

use crate::{
    config::Config,
    error::{ApiError, ApiResult},
    github_app::jwt,
};
use redis::AsyncCommands;
use serde::Deserialize;
use std::time::Duration;

const CACHE_TTL_SECS: usize = 50 * 60;
const LOCK_TTL_SECS: usize = 30;
const LOCK_POLL_INTERVAL: Duration = Duration::from_millis(200);
const LOCK_MAX_WAIT: Duration = Duration::from_secs(10);

#[derive(Debug, Deserialize)]
struct GithubTokenResponse {
    token: String,
    // expires_at is informational; we trust our own TTL.
}

fn cache_key(installation_id: i64) -> String {
    format!("gh:installation_token:{}", installation_id)
}

fn lock_key(installation_id: i64) -> String {
    format!("gh:installation_token:{}:lock", installation_id)
}

/// Get a fresh-enough installation token. Reads from Redis cache first;
/// on miss, acquires a lock, calls GitHub, writes cache, releases lock.
/// If another caller holds the lock, polls cache until it shows up.
pub async fn get(
    config: &Config,
    http: &reqwest::Client,
    redis: &mut redis::aio::ConnectionManager,
    installation_id: i64,
) -> ApiResult<String> {
    let key = cache_key(installation_id);

    if let Some(cached) = redis.get::<_, Option<String>>(&key).await? {
        return Ok(cached);
    }

    // Try to acquire the single-flight lock.
    let lock = lock_key(installation_id);
    let lock_token = uuid::Uuid::new_v4().to_string();

    let got_lock: bool = redis
        .set_options(
            &lock,
            &lock_token,
            redis::SetOptions::default()
                .conditional_set(redis::ExistenceCheck::NX)
                .with_expiration(redis::SetExpiry::EX(LOCK_TTL_SECS as u64)),
        )
        .await?;

    if got_lock {
        // We're the refresher. Mint a fresh token.
        let token = mint_from_github(config, http, installation_id).await;

        match &token {
            Ok(t) => {
                let _: () = redis
                    .set_ex(&key, t, CACHE_TTL_SECS as u64)
                    .await
                    .unwrap_or(());
            }
            Err(_) => {
                // Don't keep the lock if we failed.
                let _: () = redis.del(&lock).await.unwrap_or(());
            }
        }
        token
    } else {
        // Someone else is refreshing; poll cache.
        let start = std::time::Instant::now();
        loop {
            tokio::time::sleep(LOCK_POLL_INTERVAL).await;
            if let Some(cached) = redis.get::<_, Option<String>>(&key).await? {
                return Ok(cached);
            }
            if start.elapsed() > LOCK_MAX_WAIT {
                return Err(ApiError::Internal(anyhow::anyhow!(
                    "timed out waiting for github installation token refresh"
                )));
            }
        }
    }
}

async fn mint_from_github(
    config: &Config,
    http: &reqwest::Client,
    installation_id: i64,
) -> ApiResult<String> {
    let app_jwt = jwt::sign(&config.github_app)?;

    let url = format!(
        "https://api.github.com/app/installations/{}/access_tokens",
        installation_id
    );

    let res = http
        .post(&url)
        .bearer_auth(&app_jwt)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "coral-saas")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await?
        .error_for_status()?
        .json::<GithubTokenResponse>()
        .await?;

    Ok(res.token)
}
