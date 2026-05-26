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
    jobs,
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

    // Load the repo (RLS scope).
    let mut tx = app.db().begin().await?;
    db::set_tenant(&mut tx, tenant_id).await?;
    let repo = Repo::get_by_id(&mut tx, repo_id).await?;
    tx.commit().await?;

    let spec = JobSpec {
        job_id: Uuid::new_v4(),
        tenant_id,
        repo_id,
        kind: JobKind::Bootstrap,
        wiki_get_url: None, // First bootstrap — no prior wiki.
        wiki_put_url: None, // Worker requests pre-signed URL at run time.
        // The clone URL with installation token is minted by the worker
        // right before clone, never persisted. This placeholder is just
        // the bare repo URL; the worker substitutes the token in.
        repo_clone_url: format!("https://github.com/{}.git", repo.full_name),
        args: json!({
            "max_cost_usd": 2.00,
        }),
    };

    let job = jobs::enqueue(
        &app,
        tenant_id,
        Some(repo_id),
        Some(user.user_id),
        "bootstrap",
        json!({"max_cost_usd": 2.00}),
        spec,
    )
    .await?;

    Ok(Json(job.into()))
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
