//! Verify the `Stripe-Signature` header.
//!
//! Stripe signs the body with a secret + timestamp. We reject signatures
//! >5 minutes old (replay) and validate HMAC-SHA256.

use crate::error::ApiError;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

pub const SIGNATURE_HEADER: &str = "stripe-signature";

const MAX_AGE_SECS: i64 = 5 * 60;

/// Parse and verify a `Stripe-Signature` header like:
///   `t=1681500000,v1=abc...,v0=def...`
pub fn verify_signature(secret: &str, body: &[u8], signature_header: &str) -> Result<(), ApiError> {
    let mut timestamp: Option<i64> = None;
    let mut signatures: Vec<&str> = Vec::new();

    for part in signature_header.split(',') {
        if let Some(v) = part.strip_prefix("t=") {
            timestamp = v.parse().ok();
        } else if let Some(v) = part.strip_prefix("v1=") {
            signatures.push(v);
        }
    }

    let timestamp = timestamp.ok_or(ApiError::Forbidden)?;
    if signatures.is_empty() {
        return Err(ApiError::Forbidden);
    }

    // Reject stale signatures (replay defense).
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    if (now - timestamp).abs() > MAX_AGE_SECS {
        return Err(ApiError::Forbidden);
    }

    // Stripe signs `t.body`.
    let signed = format!("{}.{}", timestamp, String::from_utf8_lossy(body));

    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|_| ApiError::Internal(anyhow::anyhow!("invalid hmac key")))?;
    mac.update(signed.as_bytes());
    let expected = mac.finalize().into_bytes();

    for sig_hex in signatures {
        if let Ok(sig_bytes) = hex::decode(sig_hex) {
            if expected.as_slice().ct_eq(&sig_bytes).into() {
                return Ok(());
            }
        }
    }

    Err(ApiError::Forbidden)
}
