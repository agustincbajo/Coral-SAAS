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

impl JobKind {
    /// Hard wall-clock ceiling per kind (CLAUDE.md "Worker job lifecycle").
    /// The api stamps this into the JobSpec at enqueue; the worker kills
    /// the subprocess when it elapses.
    pub fn default_timeout_secs(self) -> u64 {
        match self {
            JobKind::Bootstrap => 30 * 60,
            JobKind::Ingest => 10 * 60,
            JobKind::Query => 60,
            JobKind::Lint => 10 * 60,
            JobKind::Implement => 10 * 60,
        }
    }
}

/// Spec the api enqueues for a worker. Stored in Redis (and mirrored to
/// the `jobs` Postgres row).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobSpec {
    pub job_id: Uuid,
    pub tenant_id: Uuid,
    pub repo_id: Uuid,
    pub kind: JobKind,
    /// Pre-signed GET for the previous wiki tarball, if one exists.
    /// Minted by the control plane right before enqueueing.
    pub wiki_get_url: Option<String>,
    /// Pre-signed PUT for the new wiki tarball (TTL ≥ timeout × 1.2).
    pub wiki_put_url: Option<String>,
    /// Canonical R2 key the `wiki_put_url` writes to. The worker echoes
    /// it back in `JobResult::new_wiki_key` so the control plane never
    /// has to parse it out of a pre-signed URL.
    pub wiki_tarball_key: Option<String>,
    /// Bare clone URL (no credentials). For GitHub HTTPS remotes the
    /// worker fetches a short-lived installation token via the internal
    /// api and splices it in at clone time — the token never sits in
    /// Redis. Local/file paths (tests) are cloned verbatim.
    pub repo_clone_url: String,
    /// Short-lived JWT (HS256, `WORKER_JWT_SECRET`) scoped to this job.
    /// The worker presents it as a bearer to `/api/internal/jobs/…`.
    /// **Sensitive** — never log this directly.
    pub job_token: String,
    /// Wall-clock ceiling for the subprocess, stamped by the api.
    pub timeout_secs: u64,
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
    /// Machine-readable failure class for the UI/ops (e.g.
    /// `secrets_detected`, `timeout`, `coral_exit`, `unsupported_kind`).
    pub failure_reason: Option<String>,
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
