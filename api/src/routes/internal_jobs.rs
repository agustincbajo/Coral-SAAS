//! `/api/internal/jobs/:job_id/*` — worker-facing endpoints, authenticated
//! with the short-lived per-job JWT minted at enqueue (SAAS-PLAN §9.2).
//!
//! These exist so the worker never holds long-lived GitHub or R2
//! credentials: it trades its job token for exactly the grants the job
//! needs, scoped to that job's tenant/repo and bounded by the token TTL.

use crate::{
    db::{
        self,
        models::{GithubInstallation, Job, Repo},
    },
    error::{ApiError, ApiResult},
    github_app::installation_token,
    jobs, r2,
    state::AppState,
    wiki::is_safe_slug,
};
use axum::{
    extract::{Path, State},
    http::HeaderMap,
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, time::Duration};
use uuid::Uuid;

/// One bootstrap produces at most this many wiki pages worth of pre-signed
/// URLs per request. Coral wikis are tens of pages; 500 is a generous
/// ceiling that still bounds a runaway/compromised worker.
const MAX_WIKI_PAGES_PER_REQUEST: usize = 500;
const PAGE_PUT_TTL: Duration = Duration::from_secs(15 * 60);

fn bearer(headers: &HeaderMap) -> Result<&str, ApiError> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(ApiError::Unauthorized)
}

/// Common preamble: validate the per-job bearer against the path job id,
/// load the job row, cross-check tenant, and require the job to still be
/// running. Every failure is 401/409 — this surface never reveals whether
/// a job id exists to a caller without its token.
async fn authed_running_job(
    app: &AppState,
    headers: &HeaderMap,
    job_id: Uuid,
) -> Result<Job, ApiError> {
    let token = bearer(headers)?;
    let claims = jobs::verify_job_token(&app.config().worker_jwt_secret, token, job_id)?;

    let job = Job::get_by_id(app.db(), job_id)
        .await?
        .ok_or(ApiError::Unauthorized)?;
    if job.tenant_id.to_string() != claims.tenant_id {
        return Err(ApiError::Unauthorized);
    }
    if job.status != "running" {
        return Err(ApiError::Conflict("job is not running".into()));
    }
    Ok(job)
}

#[derive(Debug, Serialize)]
struct CloneTokenDto {
    /// GitHub installation token. **Sensitive** — the worker splices it
    /// into the clone URL and forgets it; it is never logged or stored.
    token: String,
}

async fn clone_token(
    State(app): State<AppState>,
    Path(job_id): Path<Uuid>,
    headers: HeaderMap,
) -> ApiResult<Json<CloneTokenDto>> {
    let job = authed_running_job(&app, &headers, job_id).await?;
    let repo_id = job
        .repo_id
        .ok_or_else(|| ApiError::BadRequest("job has no repo".into()))?;

    let mut tx = app.db().begin().await?;
    db::set_tenant(&mut tx, job.tenant_id).await?;
    let repo = Repo::get_by_id(&mut tx, repo_id, job.tenant_id).await?;
    tx.commit().await?;

    let installation = GithubInstallation::get_by_id(app.db(), repo.installation_id)
        .await?
        .ok_or_else(|| {
            ApiError::Internal(anyhow::anyhow!(
                "repo {} has a dangling installation",
                repo_id
            ))
        })?;
    // Defense-in-depth: never mint a token for an installation that belongs
    // to a different tenant than the job. The schema does not (yet) enforce
    // repo.installation_id and repo.tenant_id agreeing, so check it here.
    if installation.tenant_id != job.tenant_id {
        return Err(ApiError::Internal(anyhow::anyhow!(
            "repo {} links an installation from another tenant",
            repo_id
        )));
    }
    if installation.suspended_at.is_some() || installation.disconnected_at.is_some() {
        return Err(ApiError::Conflict("github installation unavailable".into()));
    }

    let mut redis = app.redis();
    let token = installation_token::get(
        app.config(),
        app.http(),
        &mut redis,
        installation.installation_id,
    )
    .await?;

    Ok(Json(CloneTokenDto { token }))
}

#[derive(Debug, Deserialize)]
struct WikiUrlsRequest {
    slugs: Vec<String>,
}

#[derive(Debug, Serialize)]
struct WikiUrlsDto {
    /// slug → pre-signed PUT URL for `tenants/<t>/repos/<r>/wiki/<slug>.md`.
    urls: BTreeMap<String, String>,
}

async fn wiki_urls(
    State(app): State<AppState>,
    Path(job_id): Path<Uuid>,
    headers: HeaderMap,
    Json(req): Json<WikiUrlsRequest>,
) -> ApiResult<Json<WikiUrlsDto>> {
    let job = authed_running_job(&app, &headers, job_id).await?;
    let repo_id = job
        .repo_id
        .ok_or_else(|| ApiError::BadRequest("job has no repo".into()))?;

    if req.slugs.is_empty() {
        return Err(ApiError::BadRequest("no slugs".into()));
    }
    if req.slugs.len() > MAX_WIKI_PAGES_PER_REQUEST {
        return Err(ApiError::BadRequest(format!(
            "too many slugs (max {})",
            MAX_WIKI_PAGES_PER_REQUEST
        )));
    }
    for slug in &req.slugs {
        if !is_safe_slug(slug) {
            return Err(ApiError::BadRequest(format!("invalid slug: {slug:?}")));
        }
    }

    let r2_cfg = &app.config().r2;
    let client = r2::build_client(r2_cfg);

    let mut urls = BTreeMap::new();
    for slug in req.slugs {
        if urls.contains_key(&slug) {
            continue;
        }
        let key = format!(
            "tenants/{}/repos/{}/wiki/{}.md",
            job.tenant_id, repo_id, slug
        );
        let url = r2::presigned_put(&client, &r2_cfg.bucket, &key, PAGE_PUT_TTL)
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("presign put {}: {}", key, e)))?;
        urls.insert(slug, url);
    }

    Ok(Json(WikiUrlsDto { urls }))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/internal/jobs/:job_id/clone-token", post(clone_token))
        .route("/api/internal/jobs/:job_id/wiki-urls", post(wiki_urls))
}
