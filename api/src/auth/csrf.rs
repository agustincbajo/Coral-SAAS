//! CSRF protection — double-submit cookie pattern.
//!
//! On every state-changing request (POST/PUT/PATCH/DELETE), the client
//! reads the `csrf_token` cookie (non-HttpOnly) and echoes it back in
//! the `X-CSRF-Token` header. If both match (constant-time compare),
//! the request is genuine. If either is missing → 403.
//!
//! See SAAS-PLAN.md §8.4.

use crate::{auth::session::CSRF_COOKIE, error::ApiError};
use axum::http::HeaderMap;
use base64::Engine;
use rand::RngCore;
use subtle::ConstantTimeEq;
use tower_cookies::{cookie::SameSite, Cookie, Cookies};

pub const CSRF_HEADER: &str = "X-CSRF-Token";
const CSRF_TOKEN_BYTES: usize = 32;

/// Generate a 256-bit random CSRF token, URL-safe-base64-encoded.
pub fn generate_token() -> String {
    let mut buf = [0u8; CSRF_TOKEN_BYTES];
    rand::thread_rng().fill_bytes(&mut buf);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

/// Set the CSRF cookie. NOT HttpOnly — the SPA needs to read it from
/// JS to echo in the header. Still SameSite=Strict for defense.
pub fn build_csrf_cookie(token: String) -> Cookie<'static> {
    Cookie::build((CSRF_COOKIE, token))
        .path("/")
        .http_only(false)
        .secure(true)
        .same_site(SameSite::Strict)
        .max_age(time::Duration::days(7))
        .build()
}

/// Validate that the header matches the cookie. Use for any POST that
/// changes server state when the session is cookie-based.
pub fn validate(cookies: &Cookies, headers: &HeaderMap) -> Result<(), ApiError> {
    let cookie_val = cookies
        .get(CSRF_COOKIE)
        .map(|c| c.value().to_string())
        .ok_or_else(|| ApiError::Forbidden)?;

    let header_val = headers
        .get(CSRF_HEADER)
        .and_then(|v| v.to_str().ok())
        .ok_or(ApiError::Forbidden)?;

    if cookie_val.as_bytes().ct_eq(header_val.as_bytes()).into() {
        Ok(())
    } else {
        Err(ApiError::Forbidden)
    }
}
