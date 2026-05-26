//! Session helpers: cookie signing, extraction from a request.
//!
//! Session id is a UUID stored both as a row in `sessions` and as a
//! signed cookie value. The cookie name is `coral_session`. The cookie
//! attributes are SameSite=Strict, Secure, HttpOnly — set by `set_cookie`.

use crate::{db::models::Session, error::ApiError, state::AppState};
use axum::{
    extract::FromRequestParts,
    http::request::Parts,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use std::str::FromStr;
use tower_cookies::{cookie::SameSite, Cookie, Cookies, Key};
use uuid::Uuid;

pub const SESSION_COOKIE: &str = "coral_session";
pub const CSRF_COOKIE: &str = "csrf_token";

/// Wrapper for a session token (the cookie value). Just a Uuid under
/// the hood, but typed so we don't accidentally use any UUID as one.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct SessionToken(pub Uuid);

/// Extracted user — used as an Axum extractor on protected routes.
/// `async fn handler(user: AuthUser, ...)` → 401 if no valid session.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: Uuid,
    pub session_id: Uuid,
}

#[async_trait::async_trait]
impl FromRequestParts<AppState> for AuthUser {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let cookies = Cookies::from_request_parts(parts, state)
            .await
            .map_err(IntoResponse::into_response)?;

        let key = Key::from(state.config().session_secret.as_slice());
        let signed = cookies.signed(&key);

        let cookie = signed
            .get(SESSION_COOKIE)
            .ok_or_else(|| ApiError::Unauthorized.into_response())?;

        let session_id = Uuid::from_str(cookie.value())
            .map_err(|_| ApiError::Unauthorized.into_response())?;

        let session = Session::lookup(state.db(), session_id)
            .await
            .map_err(|e| ApiError::Database(e).into_response())?
            .ok_or_else(|| ApiError::Unauthorized.into_response())?;

        Ok(AuthUser {
            user_id: session.user_id,
            session_id: session.id,
        })
    }
}

/// Build a signed session cookie. Caller appends to the response via
/// the `Cookies` extension.
pub fn build_session_cookie(token: SessionToken) -> Cookie<'static> {
    Cookie::build((SESSION_COOKIE, token.0.to_string()))
        .path("/")
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Strict)
        .max_age(time::Duration::days(7))
        .build()
}

/// Cookie that nukes the session — set on logout.
pub fn build_clear_session_cookie() -> Cookie<'static> {
    Cookie::build((SESSION_COOKIE, ""))
        .path("/")
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Strict)
        .max_age(time::Duration::seconds(0))
        .build()
}
