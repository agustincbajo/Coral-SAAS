use serde_json::Value;
use sqlx::{FromRow, PgConnection, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct Job {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub repo_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
    pub kind: String,
    pub status: String,
    pub input: Value,
    pub output: Option<Value>,
    pub error: Option<String>,
    pub failure_reason: Option<String>,
    pub cost_usd: Option<sqlx::types::BigDecimal>,
    pub input_tokens: Option<i32>,
    pub output_tokens: Option<i32>,
    pub duration_ms: Option<i32>,
    pub queued_at: OffsetDateTime,
    pub started_at: Option<OffsetDateTime>,
    pub finished_at: Option<OffsetDateTime>,
}

impl Job {
    /// Insert into `jobs` with status='queued'. Caller pushes to Redis.
    /// Runs inside a tenant-scoped tx for RLS.
    pub async fn create(
        tx: &mut PgConnection,
        tenant_id: Uuid,
        repo_id: Option<Uuid>,
        user_id: Option<Uuid>,
        kind: &str,
        input: Value,
    ) -> Result<Self, sqlx::Error> {
        sqlx::query_as::<_, Job>(
            r#"
            INSERT INTO jobs (tenant_id, repo_id, user_id, kind, input)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id, tenant_id, repo_id, user_id, kind, status, input, output,
                      error, failure_reason, cost_usd, input_tokens, output_tokens,
                      duration_ms, queued_at, started_at, finished_at
            "#,
        )
        .bind(tenant_id)
        .bind(repo_id)
        .bind(user_id)
        .bind(kind)
        .bind(input)
        .fetch_one(tx)
        .await
    }

    /// Atomically claim a queued job by id and mark it 'running'. The
    /// worker calls this right after popping from Redis to make the
    /// transition durable (Redis pop alone is not — if the worker
    /// crashes we want to be able to detect orphaned jobs).
    ///
    /// `with_db` rather than `with_tenant` because the worker doesn't
    /// have a tenant_id context yet. RLS is bypassed via direct PgPool
    /// query — this is intentional for the worker callback path.
    pub async fn claim(pool: &PgPool, job_id: Uuid) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, Job>(
            r#"
            UPDATE jobs
            SET status = 'running', started_at = now()
            WHERE id = $1 AND status = 'queued'
            RETURNING id, tenant_id, repo_id, user_id, kind, status, input, output,
                      error, failure_reason, cost_usd, input_tokens, output_tokens,
                      duration_ms, queued_at, started_at, finished_at
            "#,
        )
        .bind(job_id)
        .fetch_optional(pool)
        .await
    }

    pub async fn complete(
        pool: &PgPool,
        job_id: Uuid,
        status: &str,
        output: Option<Value>,
        error: Option<&str>,
        failure_reason: Option<&str>,
        cost_usd: Option<f64>,
        input_tokens: Option<i32>,
        output_tokens: Option<i32>,
        duration_ms: Option<i32>,
    ) -> Result<u64, sqlx::Error> {
        // cost_usd: f64 → BigDecimal via string roundtrip to avoid
        // the bigdecimal::FromPrimitive feature requirement.
        let cost = cost_usd
            .map(|f| format!("{:.4}", f))
            .and_then(|s| s.parse::<sqlx::types::BigDecimal>().ok());

        Ok(sqlx::query(
            r#"
            UPDATE jobs
            SET status = $2,
                output = $3,
                error = $4,
                failure_reason = $5,
                cost_usd = $6,
                input_tokens = $7,
                output_tokens = $8,
                duration_ms = $9,
                finished_at = now()
            WHERE id = $1
            "#,
        )
        .bind(job_id)
        .bind(status)
        .bind(output)
        .bind(error)
        .bind(failure_reason)
        .bind(cost)
        .bind(input_tokens)
        .bind(output_tokens)
        .bind(duration_ms)
        .execute(pool)
        .await?
        .rows_affected())
    }

    pub async fn get_by_id(pool: &PgPool, id: Uuid) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, Job>(
            r#"
            SELECT id, tenant_id, repo_id, user_id, kind, status, input, output,
                   error, failure_reason, cost_usd, input_tokens, output_tokens,
                   duration_ms, queued_at, started_at, finished_at
            FROM jobs
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await
    }
}
