//! Coral-SAAS shared types.
//!
//! This crate holds types that cross the api/worker boundary: job specs,
//! job results, internal error variants, and any other contract data.
//! Keep it minimal — anything that doesn't need to be shared should live
//! in the consuming crate.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// What kind of work a job represents. The worker dispatches on this.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Bootstrap,
    Ingest,
    Query,
    Lint,
    Implement,
}

/// Job lifecycle status, written to Postgres and surfaced via SSE to the UI.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

/// Spec the api enqueues for a worker. Stored in Redis (and mirrored to
/// the `jobs` Postgres row).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobSpec {
    pub job_id: Uuid,
    pub tenant_id: Uuid,
    pub repo_id: Uuid,
    pub kind: JobKind,
    /// Pre-signed URL the worker uses to fetch the source repo metadata
    /// or wiki tarball. Mint by the control plane right before enqueueing.
    pub wiki_get_url: Option<String>,
    pub wiki_put_url: Option<String>,
    /// Repo clone URL with installation token already embedded.
    /// **Sensitive** — never log this directly.
    pub repo_clone_url: String,
    /// Arbitrary per-job arguments (e.g., `{ "question": "...", "max_cost": 2.0 }`).
    pub args: serde_json::Value,
}

/// What the worker reports back when a job completes (success or failure).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobResult {
    pub job_id: Uuid,
    pub status: JobStatus,
    /// New wiki S3 key (if the job produced one).
    pub new_wiki_key: Option<String>,
    /// Coral's stdout, parsed if JSON-able, otherwise raw.
    pub output: serde_json::Value,
    pub error: Option<String>,
    pub cost_usd: Option<f64>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub duration_ms: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid job spec: {0}")]
    InvalidJobSpec(String),

    #[error("unknown error: {0}")]
    Unknown(String),
}

/// Redis queue key — both api and worker reference this so they stay
/// in sync. One global queue for now; if we ever need per-tenant
/// queues for fair-share scheduling, swap to keyed queues.
pub const JOB_QUEUE_KEY: &str = "coral:jobs";

/// How long a worker BLPOPs before re-issuing — lets `worker-runner`
/// loop back to its heartbeat / shutdown check.
pub const WORKER_POLL_INTERVAL_SECS: usize = 30;
