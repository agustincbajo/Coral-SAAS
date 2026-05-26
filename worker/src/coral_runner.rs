//! Spawn the `coral` binary as a subprocess.
//!
//! For the MVP scaffold this is mock-mode: we return a fake result
//! so the queue-consumer pipeline is exercisable end-to-end without
//! a real `coral` binary in the image yet. When the worker Dockerfile
//! starts vendoring `coral` from the upstream release (see TODO in
//! worker/Dockerfile), swap the body for actual `Command::new`.

use shared::{JobResult, JobSpec, JobStatus};
use std::time::Duration;
use tokio::time::sleep;

const MOCK_MODE: bool = true;

pub async fn run(spec: &JobSpec) -> anyhow::Result<JobResult> {
    if MOCK_MODE {
        return run_mock(spec).await;
    }

    // ---- Real path (gated until Coral binary is vendored) ----
    //
    // 1. mkdir /tmp/job-<id>; chdir
    // 2. mint pre-signed clone URL via API → git clone --depth=1
    // 3. (if wiki_get_url Some) fetch + extract wiki.tar.zst
    // 4. (if Bootstrap) trufflehog scan; abort on verified secrets
    // 5. spawn: coral <cmd> --wiki-root .wiki/ --provider anthropic_api
    //    --max-cost <N> --json
    // 6. parse stdout (JSON when --json available; fallback to .bootstrap-state.json)
    // 7. tar -cf wiki.tar.zst .wiki/; upload via wiki_put_url
    // 8. read .bootstrap-state.json for cost_usd, input/output tokens
    // 9. return JobResult
    //
    unreachable!("real path not implemented until coral binary is vendored");
}

async fn run_mock(spec: &JobSpec) -> anyhow::Result<JobResult> {
    tracing::info!(
        job_id = %spec.job_id,
        kind = ?spec.kind,
        "[MOCK] coral subprocess simulating work (2s sleep)"
    );
    sleep(Duration::from_secs(2)).await;

    Ok(JobResult {
        job_id: spec.job_id,
        status: JobStatus::Succeeded,
        new_wiki_key: Some(format!(
            "tenants/{}/repos/{}/wiki-mock.tar.zst",
            spec.tenant_id, spec.repo_id
        )),
        output: serde_json::json!({
            "mock": true,
            "message": "Mock run — real coral binary not vendored yet"
        }),
        error: None,
        cost_usd: Some(0.0),
        input_tokens: Some(0),
        output_tokens: Some(0),
        duration_ms: 2000,
    })
}
