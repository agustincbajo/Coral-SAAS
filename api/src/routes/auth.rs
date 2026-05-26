//! `/auth/github` redirect → `/auth/github/callback` handler → logout.

use crate::{
    auth::{
        csrf::{build_csrf_cookie, generate_token},
        github_oauth,
        session::{build_clear_session_cookie, build_session_cookie, AuthUser, SessionToken},
    },
    db::models::{Session, Tenant, TenantMember, TenantRole, User},
    error::{ApiError, ApiResult},
    state::AppState,
};
use axum::{
    extract::{Query, State},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
    Router,
};
use serde::Deserialize;
use tower_cookies::{
    cookie::SameSite,
    Cookie, Cookies, Key,
};
use uuid::Uuid;

const OAUTH_STATE_COOKIE: &str = "coral_oauth_state";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/github", get(start_login))
        .route("/auth/github/callback", get(callback))
        .route("/auth/logout", post(logout))
}

/// Step 1 of OAuth — generate a fresh state nonce, stash it in a
/// short-lived cookie, and redirect to GitHub.
async fn start_login(State(state): State<AppState>, cookies: Cookies) -> Response {
    let nonce = generate_token();
    let cookie = Cookie::build((OAUTH_STATE_COOKIE, nonce.clone()))
        .path("/auth/github/callback")
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Lax)
        .max_age(time::Duration::minutes(10))
        .build();
    cookies.add(cookie);

    let url = github_oauth::authorize_url(state.config(), &nonce);
    Redirect::temporary(&url).into_response()
}

#[derive(Debug, Deserialize)]
struct CallbackQuery {
    code: String,
    state: String,
}

/// Step 2-3 of OAuth — verify `state`, exchange code for token, fetch
/// the GitHub profile, upsert the user, mint a session, set cookies.
async fn callback(
    State(app): State<AppState>,
    cookies: Cookies,
    Query(query): Query<CallbackQuery>,
) -> ApiResult<Response> {
    let stored_nonce = cookies
        .get(OAUTH_STATE_COOKIE)
        .map(|c| c.value().to_string())
        .ok_or_else(|| ApiError::BadRequest("missing oauth state cookie".into()))?;

    if stored_nonce != query.state {
        return Err(ApiError::BadRequest("oauth state mismatch".into()));
    }

    // Burn the nonce.
    cookies.remove(Cookie::from(OAUTH_STATE_COOKIE));

    let access_token =
        github_oauth::exchange_code(app.http(), app.config(), &query.code).await?;

    let gh = github_oauth::fetch_user(app.http(), &access_token).await?;

    let user = User::upsert_from_github(
        app.db(),
        gh.id,
        &gh.login,
        gh.email.as_deref(),
        gh.avatar_url.as_deref(),
    )
    .await?;

    // First-login bootstrap: ensure the user has at least one tenant
    // they own. Without this, the dashboard would land on an empty
    // state and the user couldn't do anything. Personal tenant is
    // named after their GitHub login.
    ensure_personal_tenant(app.db(), &user).await?;

    // Session regeneration (anti-fixation, GAP #31): always issue a
    // fresh session on login, never reuse a pre-login id.
    let session = Session::create(app.db(), user.id).await?;

    let key = Key::from(app.config().session_secret.as_slice());
    cookies
        .signed(&key)
        .add(build_session_cookie(SessionToken(session.id)));

    // Mint a fresh CSRF token bound to the new session.
    cookies.add(build_csrf_cookie(generate_token()));

    Ok(Redirect::to("/").into_response())
}

/// Logout — revoke the row in `sessions` and nuke the cookies.
async fn logout(
    State(app): State<AppState>,
    cookies: Cookies,
    user: AuthUser,
) -> ApiResult<Response> {
    let _: Result<u64, _> = Session::revoke(app.db(), user.session_id).await;

    cookies.add(build_clear_session_cookie());

    Ok(([("content-type", "application/json")], "{\"status\":\"ok\"}").into_response())
}

/// Ensure the user has at least one tenant they own. Idempotent — if
/// they already belong to a tenant we do nothing; if not, create a
/// personal tenant and add them as owner.
async fn ensure_personal_tenant(pool: &sqlx::PgPool, user: &User) -> ApiResult<()> {
    let existing = Tenant::list_for_user(pool, user.id).await?;
    if !existing.is_empty() {
        return Ok(());
    }

    let base_slug = sanitize_slug(&user.github_login);
    let slug = unique_slug(pool, &base_slug).await?;

    let tenant = Tenant::create(pool, &slug, &user.github_login).await?;
    TenantMember::add(pool, tenant.id, user.id, TenantRole::Owner).await?;

    tracing::info!(
        user_id = %user.id,
        tenant_id = %tenant.id,
        slug = %tenant.slug,
        "auto-created personal tenant for first-login user"
    );
    Ok(())
}

/// Render a GitHub login into a valid `tenants.slug`. Lowercase,
/// strip non-alphanumeric, trim dashes.
fn sanitize_slug(input: &str) -> String {
    let cleaned: String = input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    cleaned.trim_matches('-').to_string()
}

/// Append `-2`, `-3`, ... until the slug is unique. Race-safe enough
/// for personal tenants: the slug column is UNIQUE, so the worst case
/// is one retry per concurrent signup.
async fn unique_slug(pool: &sqlx::PgPool, base: &str) -> ApiResult<String> {
    for suffix in 0..1000 {
        let candidate = if suffix == 0 {
            base.to_string()
        } else {
            format!("{}-{}", base, suffix + 1)
        };

        let exists: Option<(i32,)> = sqlx::query_as("SELECT 1 FROM tenants WHERE slug = $1")
            .bind(&candidate)
            .fetch_optional(pool)
            .await?;

        if exists.is_none() {
            return Ok(candidate);
        }
    }
    Err(ApiError::Internal(anyhow::anyhow!(
        "exhausted slug candidates for {}", base
    )))
}

// `Uuid` import is referenced via the AuthUser fields; ensure used so
// future maintenance keeps it.
#[allow(dead_code)]
fn _force_uuid_dep(_: Uuid) {}
