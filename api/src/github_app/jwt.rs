//! Sign short-lived JWTs (10 min) using the GitHub App's private key.
//! These authenticate the App to GitHub's `/app/installations/<id>/access_tokens`
//! endpoint, which mints a 1-hour installation token we actually use.

use crate::{config::GithubAppConfig, error::ApiError};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::Serialize;

#[derive(Serialize)]
struct AppClaims {
    iat: i64,
    exp: i64,
    iss: String,
}

/// Sign a JWT good for ~9 minutes. GitHub recommends ≤10 min; we leave
/// some headroom for clock drift.
pub fn sign(config: &GithubAppConfig) -> Result<String, ApiError> {
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let claims = AppClaims {
        iat: now - 60,            // small backdate for clock skew
        exp: now + 9 * 60,
        iss: config.app_id.clone(),
    };

    let key = EncodingKey::from_rsa_pem(config.private_key_pem.as_bytes())
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("invalid GitHub App private key: {}", e)))?;

    encode(&Header::new(Algorithm::RS256), &claims, &key).map_err(ApiError::Jwt)
}
