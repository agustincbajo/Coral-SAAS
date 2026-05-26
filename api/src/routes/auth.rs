//! `/auth/github` redirect → `/auth/github/callback` handler → logout.

use crate::{
    auth::{
        csrf::{build_csrf_cookie, generate_token},
        github_oauth,
        session::{build_clear_session_cookie, build_session_cookie, AuthUser, SessionToken},
    },
    db::models::{Session, User},
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

// `Uuid` import is referenced via the AuthUser fields; ensure used so
// future maintenance keeps it.
#[allow(dead_code)]
fn _force_uuid_dep(_: Uuid) {}
