//! Database layer: pool, transaction helper, tenant scoping.

use sqlx::postgres::{PgPool, PgPoolOptions};
use std::time::Duration;

pub mod models;

/// Create the connection pool. Reasonable defaults for a small SaaS;
/// tune `max_connections` against Railway Postgres compute limits later.
pub async fn connect(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(20)
        .min_connections(2)
        .acquire_timeout(Duration::from_secs(5))
        .idle_timeout(Some(Duration::from_secs(60 * 10)))
        .max_lifetime(Some(Duration::from_secs(60 * 60)))
        .connect(database_url)
        .await
}

/// Bind `app.tenant_id` for the current transaction so RLS policies fire.
///
/// See `CLAUDE.md` and `SAAS-PLAN.md §5.5`. Caller is responsible for
/// opening the transaction and committing it. `SET LOCAL` (via
/// `set_config(..., true)`) only persists within the open tx — if
/// pgbouncer recycles the connection afterwards, no leak.
///
/// Canonical usage:
/// ```ignore
/// let mut tx = pool.begin().await?;
/// db::set_tenant(&mut tx, tenant_id).await?;
/// let rows = Repo::list_for_current_tenant(&mut tx).await?;
/// tx.commit().await?;
/// ```
pub async fn set_tenant(
    tx: &mut sqlx::PgConnection,
    tenant_id: uuid::Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(tx)
        .await?;
    Ok(())
}
