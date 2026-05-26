//! `/api/github/install/callback` — landing page after the user
//! completes the GitHub App install flow.
//!
//! GitHub redirects here with `?installation_id=N&setup_action=install`.
//! We:
//!   1. Verify the user has a session.
//!   2. Resolve the user's active tenant (single-tenant for MVP).
//!   3. Call the GitHub API to fetch the installation's account info
//!      (login, type) using a fresh installation token.
//!   4. Upsert `github_installations` linking installation_id → tenant.
//!   5. Backfill the list of accessible repos.
//!   6. Redirect the user to /dashboard/repos.
//!
//! The webhook for `installation.created` arrives independently — its
//! handler is idempotent so order-of-arrival doesn't matter.

use crate::{
    auth::AuthUser,
    audit::{self, Actor, AuditEntry},
    db::{
        self,
        models::{GithubInstallation, Repo, Tenant, TenantMember},
    },
    error::{ApiError, ApiResult},
    github_app::installation_token,
    state::AppState,
};
use axum::{
    extract::{Query, State},
    response::Redirect,
    routing::get,
    Router,
};
use serde::Deserialize;

pub fn router() -> Router<AppState> {
    Router::new().route("/api/github/install/callback", get(callback))
}

#[derive(Debug, Deserialize)]
struct CallbackQuery {
    installation_id: i64,
    // setup_action is "install" or "update" — we treat both the same.
    #[allow(dead_code)]
    setup_action: Option<String>,
}

#[derive(Debug, Deserialize)]
struct InstallationInfo {
    id: i64,
    account: AccountInfo,
}

#[derive(Debug, Deserialize)]
struct AccountInfo {
    login: String,
    #[serde(rename = "type")]
    account_type: String,
}

#[derive(Debug, Deserialize)]
struct InstallationReposResponse {
    repositories: Vec<RepoInfo>,
}

#[derive(Debug, Deserialize)]
struct RepoInfo {
    id: i64,
    full_name: String,
    default_branch: String,
}

async fn callback(
    State(app): State<AppState>,
    user: AuthUser,
    Query(query): Query<CallbackQuery>,
) -> ApiResult<Redirect> {
    let tenants = Tenant::list_for_user(app.db(), user.user_id).await?;
    let tenant = tenants
        .into_iter()
        .next()
        .ok_or_else(|| ApiError::BadRequest("user has no tenant".into()))?;

    // Verify the user is owner/admin of this tenant before linking the
    // installation (members shouldn't be able to silently take over).
    let member = TenantMember::lookup(app.db(), tenant.id, user.user_id)
        .await?
        .ok_or(ApiError::Forbidden)?;
    if member.role != "owner" && member.role != "admin" {
        return Err(ApiError::Forbidden);
    }

    // Use an app JWT to fetch installation details, then mint an
    // installation token to list repos.
    let mut redis = app.redis();
    let inst_token =
        installation_token::get(app.config(), app.http(), &mut redis, query.installation_id)
            .await?;

    let info: InstallationInfo = app
        .http()
        .get(format!(
            "https://api.github.com/app/installations/{}",
            query.installation_id
        ))
        .bearer_auth(&jwt_app_token(&app)?)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "coral-saas")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let installation = GithubInstallation::upsert(
        app.db(),
        tenant.id,
        info.id,
        &info.account.login,
        &info.account.account_type,
    )
    .await?;

    // Fetch the repositories accessible via this installation.
    let repos: InstallationReposResponse = app
        .http()
        .get("https://api.github.com/installation/repositories?per_page=100")
        .bearer_auth(&inst_token)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "coral-saas")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let mut tx = app.db().begin().await?;
    db::set_tenant(&mut tx, tenant.id).await?;
    for r in &repos.repositories {
        Repo::upsert_from_github(
            &mut tx,
            tenant.id,
            installation.id,
            r.id,
            &r.full_name,
            &r.default_branch,
        )
        .await?;
    }
    tx.commit().await?;

    audit::write(
        app.db(),
        AuditEntry {
            tenant_id: Some(tenant.id),
            actor: Actor::User(user.user_id),
            action: "github_app.installed",
            resource_kind: Some("github_installation"),
            resource_id: Some(&info.id.to_string()),
            metadata: Some(serde_json::json!({
                "account": info.account.login,
                "repos_count": repos.repositories.len(),
            })),
            legal_retention: false,
        },
    )
    .await?;

    Ok(Redirect::to("/dashboard/repos"))
}

/// Helper — short-lived app JWT for direct GitHub API calls that don't
/// need an installation token.
fn jwt_app_token(app: &AppState) -> ApiResult<String> {
    crate::github_app::jwt::sign(&app.config().github_app)
}
