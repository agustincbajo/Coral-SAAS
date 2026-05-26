//! Append-only audit log writes.
//!
//! Audit is at MVP (not post-PMF) per GAP #35. Every state-changing
//! event we care about gets a row in `audit_events`. The actor_type
//! discriminates between user actions, system jobs, operator break-glass
//! actions, and webhook-initiated changes.

use crate::error::ApiResult;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

pub enum Actor {
    User(Uuid),
    System,
    Operator(Uuid),
    WebhookGithub,
    WebhookStripe,
}

impl Actor {
    fn type_str(&self) -> &'static str {
        match self {
            Self::User(_) => "user",
            Self::System => "system",
            Self::Operator(_) => "operator",
            Self::WebhookGithub => "webhook_github",
            Self::WebhookStripe => "webhook_stripe",
        }
    }

    fn user_id(&self) -> Option<Uuid> {
        match self {
            Self::User(u) | Self::Operator(u) => Some(*u),
            _ => None,
        }
    }
}

pub struct AuditEntry<'a> {
    pub tenant_id: Option<Uuid>,
    pub actor: Actor,
    pub action: &'a str,
    pub resource_kind: Option<&'a str>,
    pub resource_id: Option<&'a str>,
    pub metadata: Option<Value>,
    pub legal_retention: bool,
}

pub async fn write(pool: &PgPool, entry: AuditEntry<'_>) -> ApiResult<()> {
    sqlx::query(
        r#"
        INSERT INTO audit_events
            (tenant_id, actor_user_id, actor_type, action,
             resource_kind, resource_id, metadata, legal_retention)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
    )
    .bind(entry.tenant_id)
    .bind(entry.actor.user_id())
    .bind(entry.actor.type_str())
    .bind(entry.action)
    .bind(entry.resource_kind)
    .bind(entry.resource_id)
    .bind(entry.metadata)
    .bind(entry.legal_retention)
    .execute(pool)
    .await?;
    Ok(())
}
