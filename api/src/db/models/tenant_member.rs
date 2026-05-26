use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TenantRole {
    Owner,
    Admin,
    Member,
}

impl TenantRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            TenantRole::Owner => "owner",
            TenantRole::Admin => "admin",
            TenantRole::Member => "member",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "owner" => Some(TenantRole::Owner),
            "admin" => Some(TenantRole::Admin),
            "member" => Some(TenantRole::Member),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct TenantMember {
    pub tenant_id: Uuid,
    pub user_id: Uuid,
    pub role: String,
    pub joined_at: OffsetDateTime,
}

impl TenantMember {
    pub async fn add(
        pool: &PgPool,
        tenant_id: Uuid,
        user_id: Uuid,
        role: TenantRole,
    ) -> Result<Self, sqlx::Error> {
        sqlx::query_as::<_, TenantMember>(
            r#"
            INSERT INTO tenant_members (tenant_id, user_id, role)
            VALUES ($1, $2, $3)
            RETURNING tenant_id, user_id, role, joined_at
            "#,
        )
        .bind(tenant_id)
        .bind(user_id)
        .bind(role.as_str())
        .fetch_one(pool)
        .await
    }

    pub async fn lookup(
        pool: &PgPool,
        tenant_id: Uuid,
        user_id: Uuid,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, TenantMember>(
            "SELECT tenant_id, user_id, role, joined_at
             FROM tenant_members
             WHERE tenant_id = $1 AND user_id = $2",
        )
        .bind(tenant_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await
    }
}
