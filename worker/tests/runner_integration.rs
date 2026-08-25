//! Hermetic integration tests for the real (non-mock) coral pipeline.
//!
//! No Docker, network, or real coral binary needed: the "remote" is a
//! local git repo, `coral` is a tiny script that honors the output
//! contract (writes `.wiki/` + `.coral/.bootstrap-state.json`, prints
//! JSON), and the control plane + R2 are one in-process axum server.

use axum::{
    extract::{Path as AxumPath, State},
    routing::{post, put},
    Json, Router,
};
use serde_json::{json, Value};
use shared::{JobKind, JobSpec, JobStatus};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
};
use uuid::Uuid;
use worker::{context::RunnerContext, coral_runner};

#[derive(Clone, Default)]
struct MockState {
    /// R2 substitute: object key → body.
    store: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
    base_url: Arc<Mutex<String>>,
}

async fn mock_wiki_urls(State(state): State<MockState>, Json(body): Json<Value>) -> Json<Value> {
    let base = state.base_url.lock().unwrap().clone();
    let mut urls = serde_json::Map::new();
    for slug in body["slugs"].as_array().cloned().unwrap_or_default() {
        let slug = slug.as_str().unwrap().to_string();
        urls.insert(slug.clone(), json!(format!("{base}/put/wiki/{slug}.md")));
    }
    Json(json!({ "urls": urls }))
}

async fn mock_clone_token() -> Json<Value> {
    Json(json!({ "token": "unused-in-tests" }))
}

async fn mock_put(
    State(state): State<MockState>,
    AxumPath(key): AxumPath<String>,
    body: axum::body::Bytes,
) -> &'static str {
    state.store.lock().unwrap().insert(key, body.to_vec());
    "ok"
}

/// Bind on an ephemeral port, serve the mock control plane + R2, return
/// (base_url, state).
async fn spawn_mock_server() -> (String, MockState) {
    let state = MockState::default();
    let app = Router::new()
        .route("/api/internal/jobs/:id/wiki-urls", post(mock_wiki_urls))
        .route("/api/internal/jobs/:id/clone-token", post(mock_clone_token))
        .route("/put/*key", put(mock_put))
        .with_state(state.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    *state.base_url.lock().unwrap() = base.clone();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, state)
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("run git");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A local "remote": one commit with a README.
fn make_remote(parent: &Path) -> PathBuf {
    let remote = parent.join("remote");
    std::fs::create_dir_all(&remote).unwrap();
    git(&remote, &["init", "--initial-branch=main"]);
    std::fs::write(remote.join("README.md"), "# Test repo\n").unwrap();
    git(&remote, &["add", "."]);
    git(
        &remote,
        &[
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "init",
        ],
    );
    remote
}

/// Write a fake `coral` whose behavior is controlled by `body_*` script
/// text per platform. Returns the path to hand to `CORAL_BIN`.
fn write_fake_coral(dir: &Path, body_unix: &str, body_windows: &str) -> PathBuf {
    if cfg!(windows) {
        let path = dir.join("coral.cmd");
        std::fs::write(&path, body_windows).unwrap();
        path
    } else {
        let path = dir.join("coral");
        std::fs::write(&path, body_unix).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }
}

const HAPPY_UNIX: &str = r#"#!/bin/sh
mkdir -p .wiki/guides .coral
printf '# Overview\n' > .wiki/overview.md
printf '# Auth Flow\n' > '.wiki/guides/Auth Flow.md'
printf '{"cost_usd": 0.42, "input_tokens": 1000, "output_tokens": 2000}' > .coral/.bootstrap-state.json
printf '{"pages": 2, "ok": true}\n'
"#;

const HAPPY_WINDOWS: &str = "@echo off\r\n\
mkdir .wiki\\guides 2>nul\r\n\
mkdir .coral 2>nul\r\n\
(echo # Overview)> .wiki\\overview.md\r\n\
(echo # Auth Flow)> \".wiki\\guides\\Auth Flow.md\"\r\n\
(echo {\"cost_usd\": 0.42, \"input_tokens\": 1000, \"output_tokens\": 2000})> .coral\\.bootstrap-state.json\r\n\
echo {\"pages\": 2, \"ok\": true}\r\n";

fn ctx_for(base_url: &str, coral_bin: &Path, work_root: &Path) -> RunnerContext {
    RunnerContext {
        http: reqwest::Client::new(),
        api_base_url: base_url.to_string(),
        coral_bin: coral_bin.to_string_lossy().to_string(),
        trufflehog_bin: "trufflehog-not-installed-in-tests".to_string(),
        work_root: work_root.to_path_buf(),
        anthropic_api_key: None,
        mock_mode: false,
    }
}

fn spec_for(remote: &Path, base_url: &str) -> JobSpec {
    JobSpec {
        job_id: Uuid::new_v4(),
        tenant_id: Uuid::new_v4(),
        repo_id: Uuid::new_v4(),
        kind: JobKind::Bootstrap,
        wiki_get_url: None,
        wiki_put_url: Some(format!("{base_url}/put/wiki.tar.zst")),
        wiki_tarball_key: Some("tenants/t/repos/r/wiki.tar.zst".to_string()),
        // Local path → the runner clones verbatim, no token fetch.
        repo_clone_url: remote.to_string_lossy().to_string(),
        job_token: "test-job-token".to_string(),
        timeout_secs: 120,
        args: json!({ "max_cost_usd": 2.0 }),
    }
}

#[tokio::test]
async fn bootstrap_happy_path_uploads_wiki() {
    let tmp = tempfile::tempdir().unwrap();
    let remote = make_remote(tmp.path());
    let coral_bin = write_fake_coral(tmp.path(), HAPPY_UNIX, HAPPY_WINDOWS);
    let (base_url, state) = spawn_mock_server().await;

    let ctx = ctx_for(&base_url, &coral_bin, tmp.path());
    let spec = spec_for(&remote, &base_url);

    let result = coral_runner::run(&ctx, &spec).await.unwrap();

    assert_eq!(
        result.status,
        JobStatus::Succeeded,
        "error: {:?}",
        result.error
    );
    assert_eq!(
        result.new_wiki_key.as_deref(),
        Some("tenants/t/repos/r/wiki.tar.zst")
    );
    assert_eq!(result.cost_usd, Some(0.42));
    assert_eq!(result.input_tokens, Some(1000));
    assert_eq!(result.output_tokens, Some(2000));
    assert_eq!(result.output["ok"], json!(true));
    assert!(result.duration_ms > 0);

    let store = state.store.lock().unwrap();
    let overview = store.get("wiki/overview.md").expect("overview uploaded");
    assert!(String::from_utf8_lossy(overview).starts_with("# Overview"));
    // `guides/Auth Flow.md` → nested path + spaces slugified.
    assert!(store.contains_key("wiki/guides-auth-flow.md"));
    let tarball = store.get("wiki.tar.zst").expect("tarball uploaded");
    assert!(!tarball.is_empty());

    // Workdir is gone.
    assert!(!tmp.path().join(format!("job-{}", spec.job_id)).exists());
}

#[tokio::test]
async fn coral_nonzero_exit_is_classified_failure() {
    let tmp = tempfile::tempdir().unwrap();
    let remote = make_remote(tmp.path());
    let coral_bin = write_fake_coral(
        tmp.path(),
        "#!/bin/sh\necho boom >&2\nexit 3\n",
        "@echo off\r\necho boom 1>&2\r\nexit /b 3\r\n",
    );
    let (base_url, _state) = spawn_mock_server().await;

    let ctx = ctx_for(&base_url, &coral_bin, tmp.path());
    let spec = spec_for(&remote, &base_url);

    let result = coral_runner::run(&ctx, &spec).await.unwrap();

    assert_eq!(result.status, JobStatus::Failed);
    assert_eq!(result.failure_reason.as_deref(), Some("coral_exit"));
    assert!(result.error.unwrap().contains("boom"));
    assert!(!tmp.path().join(format!("job-{}", spec.job_id)).exists());
}

#[tokio::test]
async fn coral_overrunning_timeout_is_killed() {
    let tmp = tempfile::tempdir().unwrap();
    let remote = make_remote(tmp.path());
    let coral_bin = write_fake_coral(
        tmp.path(),
        "#!/bin/sh\nsleep 30\n",
        "@echo off\r\nping -n 31 127.0.0.1 >nul\r\n",
    );
    let (base_url, _state) = spawn_mock_server().await;

    let ctx = ctx_for(&base_url, &coral_bin, tmp.path());
    let mut spec = spec_for(&remote, &base_url);
    spec.timeout_secs = 2;

    let result = coral_runner::run(&ctx, &spec).await.unwrap();

    assert_eq!(result.status, JobStatus::Failed);
    assert_eq!(result.failure_reason.as_deref(), Some("timeout"));
}

#[tokio::test]
async fn coral_without_wiki_output_fails_with_no_wiki() {
    let tmp = tempfile::tempdir().unwrap();
    let remote = make_remote(tmp.path());
    // Succeeds but never writes `.wiki/`.
    let coral_bin = write_fake_coral(
        tmp.path(),
        "#!/bin/sh\necho '{\"ok\":true}'\n",
        "@echo off\r\necho {\"ok\": true}\r\n",
    );
    let (base_url, _state) = spawn_mock_server().await;

    let ctx = ctx_for(&base_url, &coral_bin, tmp.path());
    let spec = spec_for(&remote, &base_url);

    let result = coral_runner::run(&ctx, &spec).await.unwrap();

    assert_eq!(result.status, JobStatus::Failed);
    assert_eq!(result.failure_reason.as_deref(), Some("no_wiki"));
}

/// Write a fake executable named `stem` (adds `.cmd` on Windows).
fn write_fake_bin(dir: &Path, stem: &str, body_unix: &str, body_windows: &str) -> PathBuf {
    if cfg!(windows) {
        let path = dir.join(format!("{stem}.cmd"));
        std::fs::write(&path, body_windows).unwrap();
        path
    } else {
        let path = dir.join(stem);
        std::fs::write(&path, body_unix).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }
}

/// A trufflehog that runs but exits non-zero (e.g. an unknown flag after a
/// version bump) must FAIL the bootstrap closed, not read as a clean scan.
#[tokio::test]
async fn trufflehog_nonzero_exit_fails_closed() {
    let tmp = tempfile::tempdir().unwrap();
    let remote = make_remote(tmp.path());
    let coral_bin = write_fake_coral(tmp.path(), HAPPY_UNIX, HAPPY_WINDOWS);
    let trufflehog = write_fake_bin(
        tmp.path(),
        "trufflehog",
        "#!/bin/sh\necho 'unknown flag' >&2\nexit 2\n",
        "@echo off\r\necho unknown flag 1>&2\r\nexit /b 2\r\n",
    );
    let (base_url, state) = spawn_mock_server().await;

    let mut ctx = ctx_for(&base_url, &coral_bin, tmp.path());
    ctx.trufflehog_bin = trufflehog.to_string_lossy().to_string();
    let spec = spec_for(&remote, &base_url);

    let result = coral_runner::run(&ctx, &spec).await.unwrap();

    assert_eq!(result.status, JobStatus::Failed);
    assert_eq!(result.failure_reason.as_deref(), Some("secret_scan_error"));
    // Coral must never have run, so nothing was uploaded.
    assert!(state.store.lock().unwrap().is_empty());
}

/// A missing trufflehog binary is the tolerated MVP skip (not a failure):
/// the bootstrap proceeds. Guards against the fail-closed change also
/// blocking the documented "scanner unavailable" path.
#[tokio::test]
async fn trufflehog_missing_is_skipped() {
    let tmp = tempfile::tempdir().unwrap();
    let remote = make_remote(tmp.path());
    let coral_bin = write_fake_coral(tmp.path(), HAPPY_UNIX, HAPPY_WINDOWS);
    let (base_url, _state) = spawn_mock_server().await;

    let mut ctx = ctx_for(&base_url, &coral_bin, tmp.path());
    ctx.trufflehog_bin = "definitely-not-a-real-binary-xyz".to_string();
    let spec = spec_for(&remote, &base_url);

    let result = coral_runner::run(&ctx, &spec).await.unwrap();
    assert_eq!(
        result.status,
        JobStatus::Succeeded,
        "error: {:?}",
        result.error
    );
}

/// If coral emits the Anthropic API key on its output, it must be scrubbed
/// before landing in `JobResult.error` (which is served to tenants).
#[tokio::test]
async fn anthropic_key_scrubbed_from_coral_output() {
    let tmp = tempfile::tempdir().unwrap();
    let remote = make_remote(tmp.path());
    let key = "sk-ant-SECRETKEY123";
    let coral_bin = write_fake_coral(
        tmp.path(),
        &format!("#!/bin/sh\necho 'auth failed with {key}' >&2\nexit 1\n"),
        &format!("@echo off\r\necho auth failed with {key} 1>&2\r\nexit /b 1\r\n"),
    );
    let (base_url, _state) = spawn_mock_server().await;

    let mut ctx = ctx_for(&base_url, &coral_bin, tmp.path());
    ctx.anthropic_api_key = Some(key.to_string());
    let spec = spec_for(&remote, &base_url);

    let result = coral_runner::run(&ctx, &spec).await.unwrap();

    assert_eq!(result.failure_reason.as_deref(), Some("coral_exit"));
    let err = result.error.unwrap();
    assert!(!err.contains(key), "API key leaked into error: {err}");
    assert!(err.contains("***"), "expected scrubbed marker, got: {err}");
}
