//! `/api/tenants/:tenant_id/repos/:repo_id/bootstrap` — kick off
//! `/api/jobs/:job_id` — status
//!
//! Authorization: requires AuthUser + tenant membership.

use crate::{
    auth::{csrf, AuthUser},
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
    http::HeaderMap,
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use serde_json::json;
use shared::{JobKind, JobSpec};
use std::time::Duration;
use tower_cookies::Cookies;
use uuid::Uuid;

/// Extra head-room, on top of a job's own timeout, for pre-signed grants to
/// survive a queue backlog before the worker claims the job. 60 min covers a
/// deep queue without making a leaked spec's write window unreasonably long.
const GRANT_QUEUE_BUFFER_SECS: u64 = 60 * 60;

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
    cookies: Cookies,
    headers: HeaderMap,
    user: AuthUser,
) -> ApiResult<Json<JobDto>> {
    // CSRF: this is a cookie-authenticated state-changing POST, so the
    // double-submit token must match (defense-in-depth atop SameSite=Strict).
    csrf::validate(&cookies, &headers)?;

    // Membership check.
    if TenantMember::lookup(app.db(), tenant_id, user.user_id)
        .await?
        .is_none()
    {
        return Err(ApiError::Forbidden);
    }

    // Load the repo (tenant-scoped) and atomically gate on bootstrap_status —
    // a second click while a bootstrap is in flight must not enqueue a
    // duplicate job that burns LLM budget. Both statements filter by
    // tenant_id explicitly (primary isolation control; RLS is inert under
    // the owner role).
    let mut tx = app.db().begin().await?;
    db::set_tenant(&mut tx, tenant_id).await?;
    let repo = Repo::get_by_id(&mut tx, repo_id, tenant_id).await?;
    let gated = sqlx::query(
        "UPDATE repos SET bootstrap_status = 'running', updated_at = now()
         WHERE id = $1 AND tenant_id = $2 AND bootstrap_status <> 'running' RETURNING id",
    )
    .bind(repo_id)
    .bind(tenant_id)
    .fetch_optional(&mut *tx)
    .await?;
    tx.commit().await?;
    if gated.is_none() {
        return Err(ApiError::Conflict("bootstrap already running".into()));
    }

    let job_id = Uuid::new_v4();
    let kind = JobKind::Bootstrap;
    let timeout_secs = kind.default_timeout_secs();
    // Grants (job token + pre-signed URLs) are minted here, at enqueue, but
    // the worker may not claim the job until it has cleared the queue. Size
    // the TTL to cover a realistic queue wait PLUS the full run, so a busy
    // queue can't expire the upload URL mid-run after a ~$2 bootstrap.
    let grant_ttl = Duration::from_secs(timeout_secs + GRANT_QUEUE_BUFFER_SECS);

    let r2_cfg = &app.config().r2;
    let r2_client = r2::build_client(r2_cfg);

    let wiki_tarball_key = format!("tenants/{}/repos/{}/wiki.tar.zst", tenant_id, repo_id);
    let wiki_put_url = r2::presigned_put(&r2_client, &r2_cfg.bucket, &wiki_tarball_key, grant_ttl)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("presign put: {}", e)))?;

    // Prior wiki (re-bootstrap) — hand the worker a GET so it can warm-start.
    // Same TTL as the other grants: a fixed 10-min window would expire while
    // the job waits in the queue and defeat the warm start.
    let wiki_get_url = match &repo.wiki_s3_key {
        Some(key) => Some(
            r2::presigned_get(&r2_client, &r2_cfg.bucket, key, grant_ttl)
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
