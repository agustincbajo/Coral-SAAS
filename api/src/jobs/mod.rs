//! Enqueue jobs: insert into `jobs` then push to Redis.
//!
//! The DB insert is the source of truth; Redis is just a fast queue.
//! If Redis drops a push (or we crash between insert and push), the
//! janitor (TODO post-MVP) re-pushes queued-but-not-running jobs.

use crate::{
    db::{self, models::Job},
    error::{ApiError, ApiResult},
    state::AppState,
};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use shared::{JobSpec, JOB_QUEUE_KEY};
use uuid::Uuid;

pub const JOB_TOKEN_AUDIENCE: &str = "worker";

/// Claims of the per-job JWT the worker presents to `/api/internal/jobs/…`.
/// Scope is a single job: `sub` is the job id, and every internal handler
/// must check the path job id against it.
#[derive(Debug, Serialize, Deserialize)]
pub struct JobTokenClaims {
    pub sub: String,
    pub tenant_id: String,
    pub aud: String,
    pub iat: i64,
    pub exp: i64,
}

/// Mint the short-lived per-job JWT (SAAS-PLAN §9.2). TTL should be the
/// job timeout × 1.2 so the token outlives the slowest legitimate run
/// but not much more.
pub fn mint_job_token(
    secret: &[u8],
    job_id: Uuid,
    tenant_id: Uuid,
    ttl_secs: u64,
) -> Result<String, jsonwebtoken::errors::Error> {
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let claims = JobTokenClaims {
        sub: job_id.to_string(),
        tenant_id: tenant_id.to_string(),
        aud: JOB_TOKEN_AUDIENCE.to_string(),
        iat: now,
        exp: now + ttl_secs as i64,
    };
    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret),
    )
}

/// Validate a per-job JWT and check it is scoped to `expected_job_id`.
/// Returns `Unauthorized` on any mismatch — internal routes must not
/// leak whether a job exists to a caller with the wrong token.
pub fn verify_job_token(
    secret: &[u8],
    bearer: &str,
    expected_job_id: Uuid,
) -> Result<JobTokenClaims, ApiError> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_audience(&[JOB_TOKEN_AUDIENCE]);
    validation.set_required_spec_claims(&["exp", "aud", "sub"]);

    let data = decode::<JobTokenClaims>(bearer, &DecodingKey::from_secret(secret), &validation)
        .map_err(|_| ApiError::Unauthorized)?;

    if data.claims.sub != expected_job_id.to_string() {
        return Err(ApiError::Unauthorized);
    }
    Ok(data.claims)
}

pub async fn enqueue(
    app: &AppState,
    tenant_id: Uuid,
    repo_id: Option<Uuid>,
    user_id: Option<Uuid>,
    kind: &str,
    input: Value,
    spec: JobSpec,
) -> ApiResult<Job> {
    // 1. Insert the row (RLS-scoped). The row id is spec.job_id — the
    //    worker claims by that id after popping the spec from Redis.
    let mut tx = app.db().begin().await?;
    db::set_tenant(&mut tx, tenant_id).await?;
    let job = Job::create(
        &mut tx,
        spec.job_id,
        tenant_id,
        repo_id,
        user_id,
        kind,
        input,
    )
    .await?;
    tx.commit().await?;

    // 2. Push to Redis.
    let mut redis = app.redis();
    let serialized = serde_json::to_string(&spec)?;
    let _: () = redis.rpush(JOB_QUEUE_KEY, serialized).await?;

    tracing::info!(job_id = %job.id, kind = %kind, "job enqueued");
    Ok(job)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"0123456789abcdef0123456789abcdef";

    #[test]
    fn job_token_roundtrip() {
        let job_id = Uuid::new_v4();
        let tenant_id = Uuid::new_v4();
        let token = mint_job_token(SECRET, job_id, tenant_id, 60).unwrap();

        let claims = verify_job_token(SECRET, &token, job_id).unwrap();
        assert_eq!(claims.sub, job_id.to_string());
        assert_eq!(claims.tenant_id, tenant_id.to_string());
        assert_eq!(claims.aud, JOB_TOKEN_AUDIENCE);
    }

    #[test]
    fn job_token_rejects_other_job() {
        let token = mint_job_token(SECRET, Uuid::new_v4(), Uuid::new_v4(), 60).unwrap();
        assert!(verify_job_token(SECRET, &token, Uuid::new_v4()).is_err());
    }

    #[test]
    fn job_token_rejects_wrong_secret() {
        let job_id = Uuid::new_v4();
        let token = mint_job_token(SECRET, job_id, Uuid::new_v4(), 60).unwrap();
        assert!(verify_job_token(b"another-secret-another-secret-xx", &token, job_id).is_err());
    }

    #[test]
    fn job_token_rejects_expired() {
        let job_id = Uuid::new_v4();
        // exp in the past beyond jsonwebtoken's default 60s leeway.
        let token = {
            let now = time::OffsetDateTime::now_utc().unix_timestamp();
            let claims = JobTokenClaims {
                sub: job_id.to_string(),
                tenant_id: Uuid::new_v4().to_string(),
                aud: JOB_TOKEN_AUDIENCE.to_string(),
                iat: now - 600,
                exp: now - 300,
            };
            encode(
                &Header::new(Algorithm::HS256),
                &claims,
                &EncodingKey::from_secret(SECRET),
            )
            .unwrap()
        };
        assert!(verify_job_token(SECRET, &token, job_id).is_err());
    }
}
