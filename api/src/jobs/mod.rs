//! Enqueue jobs: insert into `jobs` then push to Redis.
//!
//! The DB insert is the source of truth; Redis is just a fast queue. The
//! `JobSpec` (job token + pre-signed URLs) lives ONLY in Redis, so a lost
//! push cannot be reconstructed and re-pushed. Two safeguards instead:
//!   - if the push fails, we immediately mark the just-created row `failed`
//!     so it never lingers as an unrunnable `queued` orphan (see `enqueue`);
//!   - a crash in the tiny window between commit and push leaves a `queued`
//!     orphan, which the [`reap_stale_jobs`] janitor later fails out.

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

    // 2. Push to Redis. If this fails, the row is committed but will never
    //    be picked up (the spec is not persisted), so fail it out now rather
    //    than leave an unrunnable `queued` orphan.
    let mut redis = app.redis();
    let serialized = serde_json::to_string(&spec)?;
    if let Err(e) = redis.rpush::<_, _, ()>(JOB_QUEUE_KEY, serialized).await {
        let _ = Job::complete(
            app.db(),
            job.id,
            "failed",
            None,
            Some("failed to enqueue job onto the work queue"),
            Some("enqueue_failed"),
            None,
            None,
            None,
            None,
        )
        .await;
        return Err(ApiError::Redis(e));
    }

    tracing::info!(job_id = %job.id, kind = %kind, "job enqueued");
    Ok(job)
}

/// Fail out jobs (and their repos) that are stuck because their worker died.
///
/// - `running` past a generous lifetime: the worker claimed the job then
///   crashed/was killed before reporting (deploy, OOM, panic). Its own
///   subprocess timeout is ≤30min, so anything running much longer is dead.
/// - `queued` past a generous age: the enqueue committed but the Redis push
///   was lost (crash in the commit→push window), so no worker will ever see
///   it — the spec is gone and cannot be reconstructed.
///
/// Runs on a plain pool connection across all tenants — a legitimate system
/// operation. Returns the number of jobs reaped.
pub async fn reap_stale_jobs(app: &AppState) -> ApiResult<u64> {
    // 45 min > the longest job timeout (bootstrap 30 min) with headroom.
    // repo_id is nullable, so each row is an Option<Uuid>.
    let running = sqlx::query_scalar::<_, Option<Uuid>>(
        r#"
        UPDATE jobs
        SET status = 'failed',
            failure_reason = 'reaped_stale',
            error = 'worker presumed dead; job exceeded its maximum lifetime',
            finished_at = now()
        WHERE status = 'running' AND started_at < now() - interval '45 minutes'
        RETURNING repo_id
        "#,
    )
    .fetch_all(app.db())
    .await?;

    // 60 min queued with no claim → orphaned (lost push) or a pathological
    // backlog; either way it is not going to run.
    let queued = sqlx::query_scalar::<_, Option<Uuid>>(
        r#"
        UPDATE jobs
        SET status = 'failed',
            failure_reason = 'reaped_orphan',
            error = 'job was never picked up by a worker',
            finished_at = now()
        WHERE status = 'queued' AND queued_at < now() - interval '60 minutes'
        RETURNING repo_id
        "#,
    )
    .fetch_all(app.db())
    .await?;

    // Release any repos left stuck at bootstrap_status='running' by a reaped
    // job, so the owner can retry. Only touch rows still 'running'.
    let repo_ids: Vec<Uuid> = running.into_iter().chain(queued).flatten().collect();
    if !repo_ids.is_empty() {
        sqlx::query(
            "UPDATE repos SET bootstrap_status = 'failed', updated_at = now()
             WHERE id = ANY($1) AND bootstrap_status = 'running'",
        )
        .bind(&repo_ids)
        .execute(app.db())
        .await?;
    }

    let reaped = repo_ids.len() as u64;
    if reaped > 0 {
        tracing::warn!(reaped, "reaped stale/orphaned jobs");
    }
    Ok(reaped)
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
