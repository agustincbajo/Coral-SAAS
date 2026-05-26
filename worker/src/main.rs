//! Coral-SAAS worker — pulls JobSpec from Redis, dispatches to `coral`.
//!
//! On Railway this runs as a long-running service. One in-flight job
//! per replica (configurable). Each child is spawned with isolated
//! cwd/env so a rogue job can't observe sibling state. See
//! SAAS-PLAN.md §9.4.

use anyhow::Context;
use redis::{aio::ConnectionManager, AsyncCommands};
use shared::{JobKind, JobResult, JobSpec, JobStatus, JOB_QUEUE_KEY, WORKER_POLL_INTERVAL_SECS};
use sqlx::PgPool;
use std::time::Instant;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

mod coral_runner;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,worker=debug")))
        .with(tracing_subscriber::fmt::layer().json())
        .init();

    let database_url = require_env("DATABASE_URL")?;
    let redis_url = require_env("REDIS_URL")?;

    tracing::info!(redis = %mask_password(&redis_url), "coral-saas worker starting");

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .context("connect Postgres")?;

    let redis_client = redis::Client::open(redis_url)?;
    let mut redis = ConnectionManager::new(redis_client)
        .await
        .context("connect Redis")?;

    let pong: String = redis::cmd("PING").query_async(&mut redis).await?;
    tracing::info!(?pong, "redis ready");

    // How many jobs to process before suicide-restart (memory leak
    // defense, see SAAS-PLAN §9.4). Railway will restart us cleanly.
    let max_jobs: usize = std::env::var("WORKER_MAX_JOBS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100);

    let mut processed = 0usize;

    while processed < max_jobs {
        match pop_job(&mut redis).await {
            Ok(Some(spec)) => {
                let job_id = spec.job_id;
                if let Err(e) = handle_job(&pool, spec).await {
                    tracing::error!(%job_id, error = ?e, "job handling failed");
                }
                processed += 1;
            }
            Ok(None) => {
                // BLPOP timeout — no work. Keep polling.
                tracing::debug!("worker heartbeat (idle)");
            }
            Err(e) => {
                tracing::error!(error = ?e, "redis BLPOP error");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }

    tracing::info!(
        processed,
        "worker reached max_jobs ceiling, exiting for clean restart"
    );
    Ok(())
}

/// BLPOP with the configured timeout. Returns `None` on timeout.
async fn pop_job(redis: &mut ConnectionManager) -> anyhow::Result<Option<JobSpec>> {
    let result: Option<(String, String)> = redis
        .blpop(JOB_QUEUE_KEY, WORKER_POLL_INTERVAL_SECS as f64)
        .await
        .context("BLPOP")?;

    let Some((_key, payload)) = result else {
        return Ok(None);
    };

    let spec: JobSpec = serde_json::from_str(&payload).context("parse JobSpec")?;
    Ok(Some(spec))
}

async fn handle_job(pool: &PgPool, spec: JobSpec) -> anyhow::Result<()> {
    let job_id = spec.job_id;
    let kind = spec.kind;
    tracing::info!(%job_id, ?kind, "claiming job");

    // Atomic claim: queued → running. If the row was already claimed
    // (duplicate dispatch, race with shutdown, etc.) we skip.
    let claimed = sqlx::query("UPDATE jobs SET status = 'running', started_at = now()
                               WHERE id = $1 AND status = 'queued' RETURNING id")
        .bind(job_id)
        .fetch_optional(pool)
        .await?;
    if claimed.is_none() {
        tracing::warn!(%job_id, "job already claimed or unknown, skipping");
        return Ok(());
    }

    let start = Instant::now();
    let outcome = match kind {
        JobKind::Bootstrap | JobKind::Ingest | JobKind::Query | JobKind::Lint | JobKind::Implement => {
            coral_runner::run(&spec).await
        }
    };

    let duration_ms = start.elapsed().as_millis() as i32;

    match outcome {
        Ok(result) => {
            let status = match result.status {
                JobStatus::Succeeded => "succeeded",
                JobStatus::Failed => "failed",
                JobStatus::Cancelled => "cancelled",
                JobStatus::Running | JobStatus::Queued => "failed", // shouldn't happen
            };
            persist_outcome(
                pool, job_id, status, result.output, result.error.as_deref(),
                None, result.cost_usd, result.input_tokens, result.output_tokens, duration_ms,
            ).await?;
        }
        Err(e) => {
            let msg = format!("{:#}", e);
            persist_outcome(
                pool, job_id, "failed", serde_json::Value::Null, Some(&msg),
                Some("worker_panic"), None, None, None, duration_ms,
            ).await?;
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn persist_outcome(
    pool: &PgPool,
    job_id: uuid::Uuid,
    status: &str,
    output: serde_json::Value,
    error: Option<&str>,
    failure_reason: Option<&str>,
    cost_usd: Option<f64>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    duration_ms: i32,
) -> anyhow::Result<()> {
    let cost = cost_usd
        .map(|f| format!("{:.4}", f))
        .and_then(|s| s.parse::<sqlx::types::BigDecimal>().ok());

    sqlx::query(
        r#"
        UPDATE jobs
        SET status = $2, output = $3, error = $4, failure_reason = $5,
            cost_usd = $6, input_tokens = $7, output_tokens = $8,
            duration_ms = $9, finished_at = now()
        WHERE id = $1
        "#,
    )
    .bind(job_id)
    .bind(status)
    .bind(output)
    .bind(error)
    .bind(failure_reason)
    .bind(cost)
    .bind(input_tokens.map(|t| t as i32))
    .bind(output_tokens.map(|t| t as i32))
    .bind(duration_ms)
    .execute(pool)
    .await
    .context("update jobs row with outcome")?;

    Ok(())
}

fn require_env(key: &str) -> anyhow::Result<String> {
    std::env::var(key).with_context(|| format!("required env var {} not set", key))
}

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
