use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

/// Default session lifetime: 7 days absolute. Inactivity timeout is
/// enforced at the cookie/middleware layer (24h sliding); the DB row
/// is the absolute backstop.
pub const SESSION_TTL_DAYS: i64 = 7;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Session {
    pub id: Uuid,
    pub user_id: Uuid,
    pub expires_at: OffsetDateTime,
    pub created_at: OffsetDateTime,
}

impl Session {
    pub async fn create(pool: &PgPool, user_id: Uuid) -> Result<Self, sqlx::Error> {
        let expires_at = OffsetDateTime::now_utc() + Duration::days(SESSION_TTL_DAYS);
        sqlx::query_as::<_, Session>(
            r#"
            INSERT INTO sessions (user_id, expires_at)
            VALUES ($1, $2)
            RETURNING id, user_id, expires_at, created_at
            "#,
        )
        .bind(user_id)
        .bind(expires_at)
        .fetch_one(pool)
        .await
    }

    /// Look up an active session by id. Returns `None` for unknown OR
    /// expired (so the caller doesn't need to check `expires_at > now`).
    pub async fn lookup(pool: &PgPool, id: Uuid) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, Session>(
            "SELECT id, user_id, expires_at, created_at
             FROM sessions
             WHERE id = $1 AND expires_at > now()",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
    }

    pub async fn revoke(pool: &PgPool, id: Uuid) -> Result<u64, sqlx::Error> {
        Ok(sqlx::query("DELETE FROM sessions WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?
            .rows_affected())
    }

    /// Janitor: drop everything past its expiry. Called from a periodic
    /// task — keeps the `sessions` table from growing forever.
    pub async fn purge_expired(pool: &PgPool) -> Result<u64, sqlx::Error> {
        Ok(sqlx::query("DELETE FROM sessions WHERE expires_at <= now()")
            .execute(pool)
            .await?
            .rows_affected())
    }
}
