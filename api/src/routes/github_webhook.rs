//! `/api/github/webhook` — receive, validate, dispatch.
//!
//! Pipeline:
//!   1. Read raw body (we need the bytes for HMAC)
//!   2. Validate `X-Hub-Signature-256` against our secret
//!   3. Dedup via Redis on `X-GitHub-Delivery`
//!   4. Parse the JSON payload by `X-GitHub-Event` header
//!   5. Update DB rows (installations, repos), write audit_event
//!   6. Enqueue jobs (push → ingest) — stubbed until Fase 3
//!
//! See SAAS-PLAN.md §7.3 + §7.4.

use crate::{
    audit::{self, Actor, AuditEntry},
    db::models::{GithubInstallation, Repo},
    db::{self},
    error::{ApiError, ApiResult},
    github_app::webhook::{verify_signature, EventKind, DELIVERY_HEADER, EVENT_HEADER, SIGNATURE_HEADER},
    idempotency,
    state::AppState,
};
use axum::{
    body::Bytes,
    extract::State,
    http::HeaderMap,
    response::IntoResponse,
    routing::post,
    Router,
};
use serde::Deserialize;
use serde_json::Value;

pub fn router() -> Router<AppState> {
    Router::new().route("/api/github/webhook", post(webhook))
}

async fn webhook(
    State(app): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<impl IntoResponse> {
    let signature = headers
        .get(SIGNATURE_HEADER)
        .and_then(|v| v.to_str().ok())
        .ok_or(ApiError::Forbidden)?;
    verify_signature(&app.config().github_app.webhook_secret, &body, signature)?;

    let event_header = headers
        .get(EVENT_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let delivery_id = headers
        .get(DELIVERY_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let kind = EventKind::from_header(event_header);

    // Ping is a no-op smoke test from GitHub — short-circuit clean.
    if matches!(kind, EventKind::Ping) {
        return Ok(axum::http::StatusCode::OK);
    }

    // Idempotency: skip if we've already processed this delivery.
    let mut redis = app.redis();
    let first = idempotency::first_seen(&mut redis, "github_webhook", delivery_id, None).await?;
    if !first {
        tracing::info!(delivery_id, event = %event_header, "github webhook duplicate, skipping");
        return Ok(axum::http::StatusCode::OK);
    }

    let payload: Value = serde_json::from_slice(&body)?;
    tracing::info!(delivery_id, event = %event_header, "github webhook received");

    match kind {
        EventKind::Installation => handle_installation(&app, &payload).await?,
        EventKind::InstallationRepositories => {
            handle_installation_repositories(&app, &payload).await?
        }
        EventKind::Repository => handle_repository(&app, &payload).await?,
        EventKind::Push => {
            // TODO Fase 3: enqueue ingest job. For now just audit.
            audit_log_payload(&app, "push.received", &payload).await;
        }
        EventKind::PullRequest => {
            // TODO post-MVP: comment on PR if affects wiki.
            audit_log_payload(&app, "pull_request.received", &payload).await;
        }
        EventKind::Other => {
            tracing::debug!(event = %event_header, "ignoring github event");
        }
        EventKind::Ping => unreachable!("handled above"),
    }

    Ok(axum::http::StatusCode::OK)
}

// ---------- handlers ----------

#[derive(Debug, Deserialize)]
struct InstallationPayload {
    action: String,
    installation: InstallationInfo,
}

#[derive(Debug, Deserialize)]
struct InstallationInfo {
    id: i64,
    account: AccountInfo,
}

#[derive(Debug, Deserialize)]
struct AccountInfo {
    login: String,
    #[serde(rename = "type")]
    account_type: String,
}

async fn handle_installation(app: &AppState, payload: &Value) -> ApiResult<()> {
    let p: InstallationPayload = serde_json::from_value(payload.clone())?;

    match p.action.as_str() {
        "created" => {
            // installation.created arrives BEFORE the user lands on the
            // post-install redirect; we don't have a tenant_id from
            // state yet (that's threaded via the `state` query param of
            // the install URL, and surfaces in the redirect, not the
            // webhook). Until the redirect handler exists, we tolerate
            // an orphan row that gets associated at first install
            // callback. For now, log + skip until tenant linkage is wired.
            tracing::info!(
                installation_id = p.installation.id,
                account = %p.installation.account.login,
                "github installation created (awaiting tenant linkage)"
            );
        }
        "deleted" => {
            GithubInstallation::mark_disconnected(app.db(), p.installation.id).await?;
            audit::write(
                app.db(),
                AuditEntry {
                    tenant_id: None,
                    actor: Actor::WebhookGithub,
                    action: "installation.deleted",
                    resource_kind: Some("github_installation"),
                    resource_id: Some(&p.installation.id.to_string()),
                    metadata: Some(payload.clone()),
                    legal_retention: false,
                },
            )
            .await?;
        }
        "suspend" => {
            GithubInstallation::mark_suspended(app.db(), p.installation.id).await?;
        }
        "unsuspend" => {
            GithubInstallation::mark_unsuspended(app.db(), p.installation.id).await?;
        }
        other => {
            tracing::debug!(action = %other, "unhandled installation action");
        }
    }

    Ok(())
}

#[derive(Debug, Deserialize)]
struct InstallationReposPayload {
    action: String,
    installation: InstallationInfo,
    #[serde(default)]
    repositories_added: Vec<RepoRef>,
    #[serde(default)]
    repositories_removed: Vec<RepoRef>,
}

#[derive(Debug, Deserialize)]
struct RepoRef {
    id: i64,
    full_name: String,
}

async fn handle_installation_repositories(app: &AppState, payload: &Value) -> ApiResult<()> {
    let p: InstallationReposPayload = serde_json::from_value(payload.clone())?;

    let Some(installation) =
        GithubInstallation::lookup_by_installation_id(app.db(), p.installation.id).await?
    else {
        tracing::warn!(
            installation_id = p.installation.id,
            "received installation_repositories event for unknown installation"
        );
        return Ok(());
    };

    let mut tx = app.db().begin().await?;
    db::set_tenant(&mut tx, installation.tenant_id).await?;

    match p.action.as_str() {
        "added" => {
            for r in &p.repositories_added {
                // default_branch is not in the webhook payload; we'll
                // backfill via API call on first ingest. Use "main" as
                // a placeholder.
                Repo::upsert_from_github(
                    &mut tx,
                    installation.tenant_id,
                    installation.id,
                    r.id,
                    &r.full_name,
                    "main",
                )
                .await?;
            }
        }
        "removed" => {
            for r in &p.repositories_removed {
                sqlx::query(
                    "UPDATE repos SET disconnected_at = now() WHERE github_repo_id = $1",
                )
                .bind(r.id)
                .execute(&mut *tx)
                .await?;
            }
        }
        other => tracing::debug!(action = %other, "unhandled installation_repositories action"),
    }

    tx.commit().await?;
    Ok(())
}

#[derive(Debug, Deserialize)]
struct RepositoryPayload {
    action: String,
    repository: RepositoryInfo,
    installation: Option<InstallationInfo>,
}

#[derive(Debug, Deserialize)]
struct RepositoryInfo {
    id: i64,
    full_name: String,
    default_branch: Option<String>,
}

async fn handle_repository(app: &AppState, payload: &Value) -> ApiResult<()> {
    let p: RepositoryPayload = serde_json::from_value(payload.clone())?;

    let Some(inst_info) = &p.installation else {
        return Ok(());
    };
    let Some(installation) =
        GithubInstallation::lookup_by_installation_id(app.db(), inst_info.id).await?
    else {
        return Ok(());
    };

    let mut tx = app.db().begin().await?;
    db::set_tenant(&mut tx, installation.tenant_id).await?;

    match p.action.as_str() {
        "renamed" | "edited" | "transferred" => {
            sqlx::query(
                "UPDATE repos SET full_name = $1, default_branch = COALESCE($2, default_branch), updated_at = now()
                 WHERE github_repo_id = $3",
            )
            .bind(&p.repository.full_name)
            .bind(p.repository.default_branch.as_deref())
            .bind(p.repository.id)
            .execute(&mut *tx)
            .await?;
        }
        "deleted" => {
            sqlx::query(
                "UPDATE repos SET disconnected_at = now() WHERE github_repo_id = $1",
            )
            .bind(p.repository.id)
            .execute(&mut *tx)
            .await?;
        }
        _ => {}
    }

    tx.commit().await?;
    Ok(())
}

async fn audit_log_payload(app: &AppState, action: &str, payload: &Value) {
    let _ = audit::write(
        app.db(),
        AuditEntry {
            tenant_id: None,
            actor: Actor::WebhookGithub,
            action,
            resource_kind: None,
            resource_id: None,
            metadata: Some(payload.clone()),
            legal_retention: false,
        },
    )
    .await;
}
