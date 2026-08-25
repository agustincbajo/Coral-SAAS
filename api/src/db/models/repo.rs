use sqlx::{FromRow, PgConnection};
use time::OffsetDateTime;
use uuid::Uuid;

/// `Repo` does NOT derive Serialize/Deserialize — the cost column is a
/// Postgres NUMERIC (mapped to `BigDecimal`) which doesn't have a serde
/// impl by default. Route handlers should convert to a DTO when sending
/// over the wire (see `routes::repos::RepoDto`).
#[derive(Debug, Clone, FromRow)]
pub struct Repo {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub installation_id: Uuid,
    pub github_repo_id: i64,
    pub full_name: String,
    pub default_branch: String,
    pub last_indexed_sha: Option<String>,
    pub wiki_s3_key: Option<String>,
    pub embeddings_s3_key: Option<String>,
    pub bootstrap_status: String,
    pub bootstrap_cost_usd: Option<sqlx::types::BigDecimal>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub disconnected_at: Option<OffsetDateTime>,
}

impl Repo {
    /// All `Repo` queries take a `&mut PgConnection` (transaction) so
    /// the RLS policy fires. Use `db::with_tenant` to acquire the tx.
    pub async fn upsert_from_github(
        tx: &mut PgConnection,
        tenant_id: Uuid,
        installation_id: Uuid,
        github_repo_id: i64,
        full_name: &str,
        default_branch: &str,
    ) -> Result<Self, sqlx::Error> {
        sqlx::query_as::<_, Repo>(
            r#"
            INSERT INTO repos
                (tenant_id, installation_id, github_repo_id, full_name, default_branch)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (github_repo_id) DO UPDATE
                SET full_name = EXCLUDED.full_name,
                    default_branch = EXCLUDED.default_branch,
                    updated_at = now(),
                    disconnected_at = NULL
            RETURNING id, tenant_id, installation_id, github_repo_id, full_name,
                      default_branch, last_indexed_sha, wiki_s3_key, embeddings_s3_key,
                      bootstrap_status, bootstrap_cost_usd, created_at, updated_at,
                      disconnected_at
            "#,
        )
        .bind(tenant_id)
        .bind(installation_id)
        .bind(github_repo_id)
        .bind(full_name)
        .bind(default_branch)
        .fetch_one(tx)
        .await
    }

    /// List a tenant's repos. The explicit `tenant_id` predicate is the
    /// PRIMARY isolation control (CLAUDE.md invariant) — RLS is only
    /// defense-in-depth and is inert when the app connects as the table
    /// owner/superuser, so this query must never rely on it alone.
    pub async fn list_for_tenant(
        tx: &mut PgConnection,
        tenant_id: Uuid,
    ) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as::<_, Repo>(
            r#"
            SELECT id, tenant_id, installation_id, github_repo_id, full_name,
                   default_branch, last_indexed_sha, wiki_s3_key, embeddings_s3_key,
                   bootstrap_status, bootstrap_cost_usd, created_at, updated_at,
                   disconnected_at
            FROM repos
            WHERE tenant_id = $1 AND disconnected_at IS NULL
            ORDER BY created_at DESC
            "#,
        )
        .bind(tenant_id)
        .fetch_all(tx)
        .await
    }

    /// Fetch one repo, scoped to `tenant_id`. Returns `RowNotFound` (→ 404)
    /// for a repo that belongs to another tenant — the explicit filter is
    /// what enforces isolation, not RLS.
    pub async fn get_by_id(
        tx: &mut PgConnection,
        id: Uuid,
        tenant_id: Uuid,
    ) -> Result<Self, sqlx::Error> {
        sqlx::query_as::<_, Repo>(
            r#"
            SELECT id, tenant_id, installation_id, github_repo_id, full_name,
                   default_branch, last_indexed_sha, wiki_s3_key, embeddings_s3_key,
                   bootstrap_status, bootstrap_cost_usd, created_at, updated_at,
                   disconnected_at
            FROM repos
            WHERE id = $1 AND tenant_id = $2
            "#,
        )
        .bind(id)
        .bind(tenant_id)
        .fetch_one(tx)
        .await
    }
}
