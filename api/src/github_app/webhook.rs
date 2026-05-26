//! Verify GitHub webhook HMAC signatures and parse event types.
//!
//! GitHub signs the raw request body with HMAC-SHA256 and our webhook
//! secret. We MUST validate before parsing JSON — never trust the
//! event type from a header alone.

use crate::error::ApiError;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

pub const SIGNATURE_HEADER: &str = "x-hub-signature-256";
pub const EVENT_HEADER: &str = "x-github-event";
pub const DELIVERY_HEADER: &str = "x-github-delivery";

/// Verify the `X-Hub-Signature-256` header matches HMAC-SHA256(body, secret).
pub fn verify_signature(secret: &str, body: &[u8], signature: &str) -> Result<(), ApiError> {
    // GitHub format: "sha256=<hex>"
    let prefix = "sha256=";
    let sig_hex = signature
        .strip_prefix(prefix)
        .ok_or_else(|| ApiError::Forbidden)?;

    let sig_bytes = hex::decode(sig_hex).map_err(|_| ApiError::Forbidden)?;

    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|_| ApiError::Internal(anyhow::anyhow!("invalid hmac key")))?;
    mac.update(body);
    let expected = mac.finalize().into_bytes();

    if expected.as_slice().ct_eq(&sig_bytes).into() {
        Ok(())
    } else {
        Err(ApiError::Forbidden)
    }
}

/// Top-level enum of webhook events we care about. Anything else maps
/// to `Other(_)` and we ignore (logged in handler).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Installation,
    InstallationRepositories,
    Repository,
    Push,
    PullRequest,
    Ping,
    Other,
}

impl EventKind {
    pub fn from_header(s: &str) -> Self {
        match s {
            "installation" => Self::Installation,
            "installation_repositories" => Self::InstallationRepositories,
            "repository" => Self::Repository,
            "push" => Self::Push,
            "pull_request" => Self::PullRequest,
            "ping" => Self::Ping,
            _ => Self::Other,
        }
    }
}
