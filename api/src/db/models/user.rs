use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct User {
    pub id: Uuid,
    pub github_id: i64,
    pub github_login: String,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

impl User {
    /// Upsert a user by `github_id`. Returns the latest row (created or
    /// refreshed). Called on every OAuth callback so login + profile
    /// stay in sync with GitHub.
    pub async fn upsert_from_github(
        pool: &PgPool,
        github_id: i64,
        github_login: &str,
        email: Option<&str>,
        avatar_url: Option<&str>,
    ) -> Result<Self, sqlx::Error> {
        sqlx::query_as::<_, User>(
            r#"
            INSERT INTO users (github_id, github_login, email, avatar_url)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (github_id) DO UPDATE
                SET github_login = EXCLUDED.github_login,
                    email = EXCLUDED.email,
                    avatar_url = EXCLUDED.avatar_url,
                    updated_at = now()
            RETURNING id, github_id, github_login, email, avatar_url, created_at, updated_at
            "#,
        )
        .bind(github_id)
        .bind(github_login)
        .bind(email)
        .bind(avatar_url)
        .fetch_one(pool)
        .await
    }

    pub async fn get_by_id(pool: &PgPool, id: Uuid) -> Result<Self, sqlx::Error> {
        sqlx::query_as::<_, User>(
            "SELECT id, github_id, github_login, email, avatar_url, created_at, updated_at
             FROM users
             WHERE id = $1",
        )
        .bind(id)
        .fetch_one(pool)
        .await
    }
}
