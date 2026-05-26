use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct GithubInstallation {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub installation_id: i64,
    pub account_login: String,
    pub account_type: String,
    pub installed_at: OffsetDateTime,
    pub suspended_at: Option<OffsetDateTime>,
    pub disconnected_at: Option<OffsetDateTime>,
}

impl GithubInstallation {
    pub async fn upsert(
        pool: &PgPool,
        tenant_id: Uuid,
        installation_id: i64,
        account_login: &str,
        account_type: &str,
    ) -> Result<Self, sqlx::Error> {
        sqlx::query_as::<_, GithubInstallation>(
            r#"
            INSERT INTO github_installations
                (tenant_id, installation_id, account_login, account_type)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (installation_id) DO UPDATE
                SET account_login = EXCLUDED.account_login,
                    account_type = EXCLUDED.account_type,
                    suspended_at = NULL,
                    disconnected_at = NULL
            RETURNING id, tenant_id, installation_id, account_login,
                      account_type, installed_at, suspended_at, disconnected_at
            "#,
        )
        .bind(tenant_id)
        .bind(installation_id)
        .bind(account_login)
        .bind(account_type)
        .fetch_one(pool)
        .await
    }

    pub async fn lookup_by_installation_id(
        pool: &PgPool,
        installation_id: i64,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, GithubInstallation>(
            "SELECT id, tenant_id, installation_id, account_login,
                    account_type, installed_at, suspended_at, disconnected_at
             FROM github_installations
             WHERE installation_id = $1",
        )
        .bind(installation_id)
        .fetch_optional(pool)
        .await
    }

    pub async fn mark_suspended(pool: &PgPool, installation_id: i64) -> Result<u64, sqlx::Error> {
        Ok(sqlx::query(
            "UPDATE github_installations
             SET suspended_at = now()
             WHERE installation_id = $1",
        )
        .bind(installation_id)
        .execute(pool)
        .await?
        .rows_affected())
    }

    pub async fn mark_unsuspended(pool: &PgPool, installation_id: i64) -> Result<u64, sqlx::Error> {
        Ok(sqlx::query(
            "UPDATE github_installations
             SET suspended_at = NULL
             WHERE installation_id = $1",
        )
        .bind(installation_id)
        .execute(pool)
        .await?
        .rows_affected())
    }

    /// Mark for delayed purge — see §7.5 grace period (30 days). The
    /// scheduled cleanup job reads `disconnected_at + 30d <= now()`.
    pub async fn mark_disconnected(
        pool: &PgPool,
        installation_id: i64,
    ) -> Result<u64, sqlx::Error> {
        Ok(sqlx::query(
            "UPDATE github_installations
             SET disconnected_at = now()
             WHERE installation_id = $1",
        )
        .bind(installation_id)
        .execute(pool)
        .await?
        .rows_affected())
    }
}
