//! Enqueue jobs: insert into `jobs` then push to Redis.
//!
//! The DB insert is the source of truth; Redis is just a fast queue.
//! If Redis drops a push (or we crash between insert and push), the
//! janitor (TODO post-MVP) re-pushes queued-but-not-running jobs.

use crate::{
    db::{self, models::Job},
    error::ApiResult,
    state::AppState,
};
use redis::AsyncCommands;
use serde_json::Value;
use shared::{JobSpec, JOB_QUEUE_KEY};
use uuid::Uuid;

pub async fn enqueue(
    app: &AppState,
    tenant_id: Uuid,
    repo_id: Option<Uuid>,
    user_id: Option<Uuid>,
    kind: &str,
    input: Value,
    spec: JobSpec,
) -> ApiResult<Job> {
    // 1. Insert the row (RLS-scoped).
    let mut tx = app.db().begin().await?;
    db::set_tenant(&mut tx, tenant_id).await?;
    let job = Job::create(&mut tx, tenant_id, repo_id, user_id, kind, input).await?;
    tx.commit().await?;

    // 2. Push to Redis.
    let mut redis = app.redis();
    let serialized = serde_json::to_string(&spec)?;
    let _: () = redis.rpush(JOB_QUEUE_KEY, serialized).await?;

    tracing::info!(job_id = %job.id, kind = %kind, "job enqueued");
    Ok(job)
}
