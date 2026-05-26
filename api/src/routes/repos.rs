//! `/api/tenants/:tenant_id/repos` — list a tenant's repos.
//!
//! This route runs through `db::with_tenant` so the RLS policy fires.
//! Authorization (caller is a member of `tenant_id`) is checked first
//! via `TenantMember::lookup`.

use crate::{
    auth::AuthUser,
    db::{self, models::Repo, models::TenantMember},
    error::{ApiError, ApiResult},
    state::AppState,
};
use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Serialize)]
struct RepoDto {
    id: Uuid,
    full_name: String,
    default_branch: String,
    bootstrap_status: String,
    last_indexed_sha: Option<String>,
}

async fn list_repos(
    State(app): State<AppState>,
    Path(tenant_id): Path<Uuid>,
    user: AuthUser,
) -> ApiResult<Json<Vec<RepoDto>>> {
    // Membership check — short-circuit before the tenant-scoped tx.
    let member = TenantMember::lookup(app.db(), tenant_id, user.user_id).await?;
    if member.is_none() {
        return Err(ApiError::Forbidden);
    }

    let mut tx = app.db().begin().await?;
    db::set_tenant(&mut tx, tenant_id).await?;
    let repos = Repo::list_for_current_tenant(&mut tx).await?;
    tx.commit().await?;

    Ok(Json(
        repos
            .into_iter()
            .map(|r| RepoDto {
                id: r.id,
                full_name: r.full_name,
                default_branch: r.default_branch,
                bootstrap_status: r.bootstrap_status,
                last_indexed_sha: r.last_indexed_sha,
            })
            .collect(),
    ))
}

pub fn router() -> Router<AppState> {
    Router::new().route("/api/tenants/:tenant_id/repos", get(list_repos))
}
