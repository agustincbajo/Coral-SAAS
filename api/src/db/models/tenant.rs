use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgConnection, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum TenantPlan {
    Free,
    Pro,
    Team,
    Enterprise,
}

impl TenantPlan {
    pub fn as_str(&self) -> &'static str {
        match self {
            TenantPlan::Free => "free",
            TenantPlan::Pro => "pro",
            TenantPlan::Team => "team",
            TenantPlan::Enterprise => "enterprise",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Tenant {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub plan: String,
    pub stripe_customer_id: Option<String>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub deleted_at: Option<OffsetDateTime>,
}

impl Tenant {
    pub async fn create(
        pool: &PgPool,
        slug: &str,
        name: &str,
    ) -> Result<Self, sqlx::Error> {
        sqlx::query_as::<_, Tenant>(
            r#"
            INSERT INTO tenants (slug, name)
            VALUES ($1, $2)
            RETURNING id, slug, name, plan, stripe_customer_id,
                      created_at, updated_at, deleted_at
            "#,
        )
        .bind(slug)
        .bind(name)
        .fetch_one(pool)
        .await
    }

    /// Fetch from within a tenant-scoped transaction. RLS enforces the
    /// scope; we just SELECT.
    pub async fn get_current(tx: &mut PgConnection) -> Result<Self, sqlx::Error> {
        sqlx::query_as::<_, Tenant>(
            r#"
            SELECT id, slug, name, plan, stripe_customer_id,
                   created_at, updated_at, deleted_at
            FROM tenants
            WHERE id = current_setting('app.tenant_id', true)::uuid
              AND deleted_at IS NULL
            "#,
        )
        .fetch_one(tx)
        .await
    }

    /// List tenants a user is a member of. Bypasses RLS via direct
    /// `users` + `tenant_members` join (no tenant filter required since
    /// we filter by user_id).
    pub async fn list_for_user(
        pool: &PgPool,
        user_id: Uuid,
    ) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as::<_, Tenant>(
            r#"
            SELECT t.id, t.slug, t.name, t.plan, t.stripe_customer_id,
                   t.created_at, t.updated_at, t.deleted_at
            FROM tenants t
            INNER JOIN tenant_members tm ON tm.tenant_id = t.id
            WHERE tm.user_id = $1
              AND t.deleted_at IS NULL
            ORDER BY t.created_at ASC
            "#,
        )
        .bind(user_id)
        .fetch_all(pool)
        .await
    }
}
