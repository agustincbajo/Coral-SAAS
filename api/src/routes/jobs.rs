//! `/api/tenants/:tenant_id/repos/:repo_id/bootstrap` — kick off
//! `/api/jobs/:job_id` — status
//!
//! Authorization: requires AuthUser + tenant membership.

use crate::{
    auth::AuthUser,
    db::{
        self,
        models::{Job, Repo, TenantMember},
    },
    error::{ApiError, ApiResult},
    jobs, r2,
    state::AppState,
};
use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use serde_json::json;
use shared::{JobKind, JobSpec};
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Serialize)]
struct JobDto {
    id: Uuid,
    kind: String,
    status: String,
    error: Option<String>,
    failure_reason: Option<String>,
    queued_at: time::OffsetDateTime,
    started_at: Option<time::OffsetDateTime>,
    finished_at: Option<time::OffsetDateTime>,
}

impl From<Job> for JobDto {
    fn from(j: Job) -> Self {
        Self {
            id: j.id,
            kind: j.kind,
            status: j.status,
            error: j.error,
            failure_reason: j.failure_reason,
            queued_at: j.queued_at,
            started_at: j.started_at,
            finished_at: j.finished_at,
        }
    }
}

async fn start_bootstrap(
    State(app): State<AppState>,
    Path((tenant_id, repo_id)): Path<(Uuid, Uuid)>,
    user: AuthUser,
) -> ApiResult<Json<JobDto>> {
    // Membership check.
    if TenantMember::lookup(app.db(), tenant_id, user.user_id)
        .await?
        .is_none()
    {
        return Err(ApiError::Forbidden);
    }

    // Load the repo (RLS scope) and atomically gate on bootstrap_status —
    // a second click while a bootstrap is in flight must not enqueue a
    // duplicate job that burns LLM budget.
    let mut tx = app.db().begin().await?;
    db::set_tenant(&mut tx, tenant_id).await?;
    let repo = Repo::get_by_id(&mut tx, repo_id).await?;
    let gated = sqlx::query(
        "UPDATE repos SET bootstrap_status = 'running', updated_at = now()
         WHERE id = $1 AND bootstrap_status <> 'running' RETURNING id",
    )
    .bind(repo_id)
    .fetch_optional(&mut *tx)
    .await?;
    tx.commit().await?;
    if gated.is_none() {
        return Err(ApiError::Conflict("bootstrap already running".into()));
    }

    let job_id = Uuid::new_v4();
    let kind = JobKind::Bootstrap;
    let timeout_secs = kind.default_timeout_secs();
    // Token + PUT URL must outlive the slowest legitimate run (×1.2).
    let grant_ttl = Duration::from_secs(timeout_secs * 12 / 10);

    let r2_cfg = &app.config().r2;
    let r2_client = r2::build_client(r2_cfg);

    let wiki_tarball_key = format!("tenants/{}/repos/{}/wiki.tar.zst", tenant_id, repo_id);
    let wiki_put_url = r2::presigned_put(&r2_client, &r2_cfg.bucket, &wiki_tarball_key, grant_ttl)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("presign put: {}", e)))?;

    // Prior wiki (re-bootstrap) — hand the worker a GET so it can warm-start.
    let wiki_get_url = match &repo.wiki_s3_key {
        Some(key) => Some(
            r2::presigned_get(
                &r2_client,
                &r2_cfg.bucket,
                key,
                Duration::from_secs(10 * 60),
            )
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("presign get: {}", e)))?,
        ),
        None => None,
    };

    let job_token = jobs::mint_job_token(
        &app.config().worker_jwt_secret,
        job_id,
        tenant_id,
        grant_ttl.as_secs(),
    )?;

    let spec = JobSpec {
        job_id,
        tenant_id,
        repo_id,
        kind,
        wiki_get_url,
        wiki_put_url: Some(wiki_put_url),
        wiki_tarball_key: Some(wiki_tarball_key),
        // Bare URL — the worker fetches an installation token through
        // `/api/internal/jobs/:id/clone-token` and splices it in at
        // clone time, so no credential ever sits in Redis.
        repo_clone_url: format!("https://github.com/{}.git", repo.full_name),
        job_token,
        timeout_secs,
        args: json!({
            "max_cost_usd": 2.00,
        }),
    };

    let enqueued = jobs::enqueue(
        &app,
        tenant_id,
        Some(repo_id),
        Some(user.user_id),
        "bootstrap",
        json!({"max_cost_usd": 2.00}),
        spec,
    )
    .await;

    match enqueued {
        Ok(job) => Ok(Json(job.into())),
        Err(e) => {
            // Release the gate so the user can retry; the enqueue never
            // reached the queue.
            let mut tx = app.db().begin().await?;
            db::set_tenant(&mut tx, tenant_id).await?;
            sqlx::query(
                "UPDATE repos SET bootstrap_status = 'failed', updated_at = now() WHERE id = $1",
            )
            .bind(repo_id)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            Err(e)
        }
    }
}

async fn get_job(
    State(app): State<AppState>,
    Path(job_id): Path<Uuid>,
    user: AuthUser,
) -> ApiResult<Json<JobDto>> {
    let job = Job::get_by_id(app.db(), job_id)
        .await?
        .ok_or(ApiError::NotFound)?;

    // Authorization: must be a member of the job's tenant.
    if TenantMember::lookup(app.db(), job.tenant_id, user.user_id)
        .await?
        .is_none()
    {
        return Err(ApiError::Forbidden);
    }

    Ok(Json(job.into()))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/tenants/:tenant_id/repos/:repo_id/bootstrap",
            post(start_bootstrap),
        )
        .route("/api/jobs/:job_id", get(get_job))
}
