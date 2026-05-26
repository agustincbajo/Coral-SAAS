//! `/api/tenants/:tenant_id/repos/:repo_id/wiki/:slug` — fetch one
//! page from R2, render, return JSON `{ slug, title, html }`.
//!
//! Fallback: if the object doesn't exist yet (bootstrap not finished),
//! return 404. Frontend treats that as "wiki not ready".

use crate::{
    auth::AuthUser,
    db::{self, models::TenantMember},
    error::{ApiError, ApiResult},
    state::AppState,
    wiki::render::render_markdown,
    r2,
};
use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Serialize)]
struct WikiPageDto {
    slug: String,
    title: Option<String>,
    html: String,
}

async fn get_page(
    State(app): State<AppState>,
    Path((tenant_id, repo_id, slug)): Path<(Uuid, Uuid, String)>,
    user: AuthUser,
) -> ApiResult<Json<WikiPageDto>> {
    if TenantMember::lookup(app.db(), tenant_id, user.user_id)
        .await?
        .is_none()
    {
        return Err(ApiError::Forbidden);
    }

    // Validate slug shape. Wiki slugs are constrained to safe chars by
    // Coral's own SCHEMA — but we belt-and-suspenders here so a path
    // traversal attempt (`..` etc.) doesn't reach R2.
    if !is_safe_slug(&slug) {
        return Err(ApiError::BadRequest("invalid slug".into()));
    }

    // Verify the repo belongs to this tenant via the RLS-scoped tx.
    let mut tx = app.db().begin().await?;
    db::set_tenant(&mut tx, tenant_id).await?;
    let _ = crate::db::models::Repo::get_by_id(&mut tx, repo_id).await?;
    tx.commit().await?;

    let key = format!("tenants/{}/repos/{}/wiki/{}.md", tenant_id, repo_id, slug);

    let client = r2::build_client(&app.config().r2);
    let bytes = match r2::get_object(&client, &app.config().r2.bucket, &key).await {
        Ok(b) => b,
        Err(r2::R2Error::NotFound) => return Err(ApiError::NotFound),
        Err(e) => return Err(ApiError::Internal(anyhow::anyhow!("{}", e))),
    };

    let md = String::from_utf8(bytes).map_err(|e| {
        ApiError::Internal(anyhow::anyhow!("wiki page is not UTF-8: {}", e))
    })?;

    let rendered = render_markdown(&md);

    Ok(Json(WikiPageDto {
        slug,
        title: rendered.title,
        html: rendered.html,
    }))
}

/// Allowed: `[a-z0-9-]+`. Coral's own SCHEMA uses kebab-case slugs.
fn is_safe_slug(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 200
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/tenants/:tenant_id/repos/:repo_id/wiki/:slug",
        get(get_page),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_validation() {
        assert!(is_safe_slug("auth-flow"));
        assert!(is_safe_slug("a"));
        assert!(is_safe_slug("user-model-2"));
        assert!(!is_safe_slug(""));
        assert!(!is_safe_slug("../etc/passwd"));
        assert!(!is_safe_slug("Foo"));
        assert!(!is_safe_slug("auth_flow"));
        assert!(!is_safe_slug("auth flow"));
    }
}
