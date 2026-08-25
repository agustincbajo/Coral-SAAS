//! Run one job end-to-end: clone → (restore wiki) → secret-scan →
//! `coral` subprocess → package `.wiki/` → upload to R2 via pre-signed
//! URLs. See SAAS-PLAN §9.2/§9.4.
//!
//! Failure philosophy: anything that is a *property of the job* (secrets
//! found, coral non-zero exit, timeout, unsupported kind) returns
//! `Ok(JobResult{status: Failed, failure_reason, ..})` so it lands in the
//! jobs row as a classified failure. Only unexpected infra errors bubble
//! as `Err` (the caller records those as `worker_panic`).
//!
//! Secret hygiene: installation tokens travel in an `Authorization`
//! header (never the clone URL), pre-signed URLs are scrubbed from
//! reqwest errors via `without_url()`, and subprocess output is scrubbed
//! against the token before it can reach logs or the jobs row.

use crate::context::RunnerContext;
use base64::Engine;
use serde_json::{json, Value};
use shared::{JobKind, JobResult, JobSpec, JobStatus};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::{io::AsyncReadExt, process::Command, time::sleep};

const CLONE_TIMEOUT_SECS: u64 = 10 * 60;
const TRUFFLEHOG_TIMEOUT_SECS: u64 = 5 * 60;
/// Wiki pages larger than this are skipped with a warning — the render
/// endpoint reads whole objects into memory.
const MAX_PAGE_BYTES: u64 = 5 * 1024 * 1024;
const COST_KEYS: &[&str] = &["cost_usd", "total_cost_usd", "total_cost"];
const INPUT_TOKEN_KEYS: &[&str] = &["input_tokens", "total_input_tokens"];
const OUTPUT_TOKEN_KEYS: &[&str] = &["output_tokens", "total_output_tokens"];

pub async fn run(ctx: &RunnerContext, spec: &JobSpec) -> anyhow::Result<JobResult> {
    if ctx.mock_mode {
        return run_mock(spec).await;
    }

    let started = Instant::now();
    let workdir = ctx.work_root.join(format!("job-{}", spec.job_id));

    // Fresh workdir per job; stale leftovers from a crashed run are junk.
    let _ = tokio::fs::remove_dir_all(&workdir).await;
    tokio::fs::create_dir_all(&workdir)
        .await
        .map_err(|e| anyhow::anyhow!("create workdir: {}", e))?;

    let outcome = run_real(ctx, spec, &workdir).await;

    if let Err(e) = tokio::fs::remove_dir_all(&workdir).await {
        tracing::warn!(job_id = %spec.job_id, error = %e, "workdir cleanup failed");
    }

    let mut result = outcome?;
    result.duration_ms = started.elapsed().as_millis() as i64;
    Ok(result)
}

async fn run_real(
    ctx: &RunnerContext,
    spec: &JobSpec,
    workdir: &Path,
) -> anyhow::Result<JobResult> {
    let repo_dir = workdir.join("src");

    // ---- 1. Clone (shallow) ----
    let auth = resolve_clone_auth(ctx, spec).await?;
    let clone = git_clone(spec, &auth, &repo_dir).await?;
    if clone.timed_out {
        return Ok(failed(spec, "timeout", "git clone timed out"));
    }
    if !clone.success {
        // stderr is already scrubbed of the token by run_cmd.
        return Ok(failed(
            spec,
            "clone_failed",
            &format!("git clone failed: {}", tail(&clone.stderr, 2000)),
        ));
    }

    // ---- 2. Restore previous wiki, if the control plane handed us one ----
    let wiki_dir = repo_dir.join(".wiki");
    if let Some(url) = &spec.wiki_get_url {
        match download(ctx, url).await {
            Ok(bytes) => {
                let dest = repo_dir.clone();
                tokio::task::spawn_blocking(move || extract_tar_zst(&bytes, &dest)).await??;
                tracing::info!(job_id = %spec.job_id, "previous wiki restored");
            }
            // Non-fatal: a missing/expired prior tarball just means a cold
            // bootstrap.
            Err(e) => {
                tracing::warn!(job_id = %spec.job_id, error = %e, "prior wiki fetch failed, cold start")
            }
        }
    }

    // ---- 3. Secret scan gate (bootstrap only — GAP #68) ----
    //
    // Fail-closed on scan ERROR: a scanner that ran but exited non-zero (or
    // timed out) is NOT evidence of a clean repo, so we abort rather than
    // hand a possibly-secret-laden tree to the LLM. Only a genuinely
    // missing/unspawnable binary is treated as "skip" — the documented MVP
    // concession while GAP #68 is not fully closed.
    if spec.kind == JobKind::Bootstrap {
        match trufflehog_scan(ctx, &repo_dir).await {
            Ok(SecretScan::Clean) => {}
            Ok(SecretScan::Findings(n)) => {
                return Ok(failed(
                    spec,
                    "secrets_detected",
                    &format!("{n} verified secret(s) detected in the repository; aborting so they never reach an LLM"),
                ));
            }
            Ok(SecretScan::Unavailable(why)) => {
                tracing::warn!(job_id = %spec.job_id, reason = %why, "trufflehog unavailable, skipping secret scan")
            }
            Err(e) => {
                return Ok(failed(
                    spec,
                    "secret_scan_error",
                    &format!("secret scan failed to complete: {e}"),
                ));
            }
        }
    }

    // ---- 4. coral subprocess ----
    let mut args: Vec<String> = match spec.kind {
        JobKind::Bootstrap => vec!["bootstrap".into()],
        JobKind::Ingest => vec!["ingest".into()],
        JobKind::Query => {
            let question = spec
                .args
                .get("question")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if question.is_empty() {
                return Ok(failed(spec, "invalid_args", "query job without a question"));
            }
            vec!["query".into(), question.into()]
        }
        JobKind::Lint | JobKind::Implement => {
            return Ok(failed(
                spec,
                "unsupported_kind",
                "job kind not supported by this worker yet",
            ));
        }
    };
    args.extend([
        "--wiki-root".into(),
        ".wiki".into(),
        "--provider".into(),
        "anthropic_api".into(),
        "--json".into(),
    ]);
    if let Some(max_cost) = spec.args.get("max_cost_usd").and_then(Value::as_f64) {
        args.extend(["--max-cost".into(), format!("{max_cost}")]);
    }

    let mut cmd = Command::new(&ctx.coral_bin);
    cmd.args(&args).current_dir(&repo_dir);
    if let Some(key) = &ctx.anthropic_api_key {
        cmd.env("ANTHROPIC_API_KEY", key);
    }
    // Scrub the API key from coral's captured output before it can reach
    // the jobs row (served back to tenants via GET /api/jobs/:id) or the
    // logs — same defense-in-depth the clone path applies to its token.
    let coral_scrub: Vec<&str> = ctx.anthropic_api_key.as_deref().into_iter().collect();
    let coral = run_cmd(cmd, Duration::from_secs(spec.timeout_secs), &coral_scrub).await?;
    if coral.timed_out {
        return Ok(failed(
            spec,
            "timeout",
            &format!("coral exceeded the {}s job timeout", spec.timeout_secs),
        ));
    }
    if !coral.success {
        return Ok(failed(
            spec,
            "coral_exit",
            &format!("coral exited non-zero: {}", tail(&coral.stderr, 2000)),
        ));
    }

    // Coral doesn't guarantee JSON on every subcommand; keep the raw tail
    // when it isn't.
    let output: Value = parse_json_output(&coral.stdout)
        .unwrap_or_else(|| json!({ "raw": tail(&coral.stdout, 4000) }));

    // Cost accounting: prefer the bootstrap state file, fall back to stdout.
    let state = read_state_file(&repo_dir).await;
    let source = state.as_ref().unwrap_or(&output);
    let cost_usd = find_number(source, COST_KEYS).or_else(|| find_number(&output, COST_KEYS));
    let input_tokens = find_number(source, INPUT_TOKEN_KEYS)
        .or_else(|| find_number(&output, INPUT_TOKEN_KEYS))
        .map(|f| f as i64);
    let output_tokens = find_number(source, OUTPUT_TOKEN_KEYS)
        .or_else(|| find_number(&output, OUTPUT_TOKEN_KEYS))
        .map(|f| f as i64);

    // ---- 5. Package + upload (kinds that mutate the wiki) ----
    let mut new_wiki_key = None;
    if matches!(spec.kind, JobKind::Bootstrap | JobKind::Ingest) {
        if !wiki_dir.is_dir() {
            return Ok(failed(
                spec,
                "no_wiki",
                "coral succeeded but produced no .wiki/ directory",
            ));
        }

        let pages = collect_pages(&wiki_dir)?;
        if pages.is_empty() {
            return Ok(failed(
                spec,
                "no_wiki",
                "coral succeeded but .wiki/ has no markdown pages",
            ));
        }

        // 5a. Tarball → pre-signed PUT from the spec (backup/portability).
        if let Some(put_url) = &spec.wiki_put_url {
            let parent = repo_dir.clone();
            let tarball =
                tokio::task::spawn_blocking(move || create_tar_zst(&parent, ".wiki")).await??;
            upload_put(ctx, put_url, tarball, "application/zstd").await?;
            new_wiki_key = spec.wiki_tarball_key.clone();
        }

        // 5b. Individual pages → URLs minted on demand by the control
        // plane (the api can't know slugs at enqueue time).
        let slugs: Vec<String> = pages.iter().map(|(slug, _)| slug.clone()).collect();
        let urls = request_wiki_urls(ctx, spec, &slugs).await?;
        for (slug, path) in &pages {
            let Some(url) = urls.get(slug) else {
                tracing::warn!(job_id = %spec.job_id, slug, "no pre-signed URL for page, skipping");
                continue;
            };
            let bytes = tokio::fs::read(path).await?;
            upload_put(ctx, url, bytes, "text/markdown; charset=utf-8").await?;
        }
        tracing::info!(job_id = %spec.job_id, pages = pages.len(), "wiki uploaded");
    }

    Ok(JobResult {
        job_id: spec.job_id,
        status: JobStatus::Succeeded,
        new_wiki_key,
        output,
        error: None,
        failure_reason: None,
        cost_usd,
        input_tokens,
        output_tokens,
        duration_ms: 0, // stamped by `run`
    })
}

// ---------------------------------------------------------------------------
// Clone

struct CloneAuth {
    /// Bare URL, no credentials.
    url: String,
    /// `http.extraheader=` value carrying the installation token, plus the
    /// raw token for output scrubbing. `None` for local paths (tests) and
    /// non-GitHub remotes.
    header: Option<(String, String)>,
}

async fn resolve_clone_auth(ctx: &RunnerContext, spec: &JobSpec) -> anyhow::Result<CloneAuth> {
    let url = spec.repo_clone_url.clone();
    if !url.starts_with("https://github.com/") {
        // Local path or non-GitHub remote — clone verbatim, no token.
        return Ok(CloneAuth { url, header: None });
    }

    let token = fetch_clone_token(ctx, spec).await?;
    // Same scheme actions/checkout uses: basic auth via header, so the
    // token never appears in the URL (git echoes URLs into errors and
    // stores them in .git/config).
    let basic = base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{token}"));
    Ok(CloneAuth {
        url,
        header: Some((format!("Authorization: basic {basic}"), token)),
    })
}

async fn fetch_clone_token(ctx: &RunnerContext, spec: &JobSpec) -> anyhow::Result<String> {
    let value = internal_post(ctx, spec, "clone-token").await?;
    value
        .get("token")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("clone-token response missing token field"))
}

async fn git_clone(spec: &JobSpec, auth: &CloneAuth, dest: &Path) -> anyhow::Result<CmdOutput> {
    let mut cmd = Command::new("git");
    let mut scrub: Vec<String> = Vec::new();
    if let Some((header, token)) = &auth.header {
        cmd.arg("-c").arg(format!("http.extraheader={header}"));
        scrub.push(token.clone());
        scrub.push(header.clone());
    }
    cmd.arg("clone")
        .arg("--depth")
        .arg("1")
        .arg(&auth.url)
        .arg(dest)
        .env("GIT_TERMINAL_PROMPT", "0");

    let timeout = Duration::from_secs(CLONE_TIMEOUT_SECS.min(spec.timeout_secs.max(60)));
    let scrub_refs: Vec<&str> = scrub.iter().map(String::as_str).collect();
    run_cmd(cmd, timeout, &scrub_refs).await
}

// ---------------------------------------------------------------------------
// Subprocess plumbing

struct CmdOutput {
    success: bool,
    timed_out: bool,
    stdout: String,
    stderr: String,
}

/// Spawn with a hard timeout; the child is killed if the deadline passes
/// (`kill_on_drop`). `scrub` values are replaced with `***` in captured
/// output before anyone can log or persist it.
async fn run_cmd(mut cmd: Command, timeout: Duration, scrub: &[&str]) -> anyhow::Result<CmdOutput> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let program = cmd.as_std().get_program().to_string_lossy().to_string();
    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow::anyhow!("spawn {}: {}", program, e))?;

    let mut stdout_pipe = child.stdout.take().expect("stdout piped");
    let mut stderr_pipe = child.stderr.take().expect("stderr piped");
    let reader = async {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let _ = tokio::join!(
            stdout_pipe.read_to_end(&mut out),
            stderr_pipe.read_to_end(&mut err)
        );
        (out, err)
    };

    let outcome = tokio::time::timeout(timeout, async {
        let ((out, err), status) = tokio::join!(reader, child.wait());
        (out, err, status)
    })
    .await;

    match outcome {
        Ok((out, err, status)) => {
            let status = status.map_err(|e| anyhow::anyhow!("wait {}: {}", program, e))?;
            Ok(CmdOutput {
                success: status.success(),
                timed_out: false,
                stdout: scrub_secrets(&String::from_utf8_lossy(&out), scrub),
                stderr: scrub_secrets(&String::from_utf8_lossy(&err), scrub),
            })
        }
        Err(_) => {
            let _ = child.kill().await;
            Ok(CmdOutput {
                success: false,
                timed_out: true,
                stdout: String::new(),
                stderr: String::new(),
            })
        }
    }
}

fn scrub_secrets(s: &str, secrets: &[&str]) -> String {
    let mut out = s.to_string();
    for secret in secrets {
        if !secret.is_empty() {
            out = out.replace(secret, "***");
        }
    }
    out
}

fn tail(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.trim().to_string();
    }
    let start = s.len() - max;
    // Don't split a UTF-8 codepoint.
    let start = (start..s.len())
        .find(|i| s.is_char_boundary(*i))
        .unwrap_or(start);
    format!("…{}", s[start..].trim())
}

// ---------------------------------------------------------------------------
// trufflehog

/// Outcome of the secret scan. The distinction between `Unavailable` (the
/// scanner could not be spawned at all) and an `Err` return (it ran but
/// failed / timed out) is load-bearing: the former is a tolerated MVP skip,
/// the latter fails the job closed. `Ok(Clean)` is the ONLY "proceed"
/// signal — a non-zero exit must never masquerade as a clean scan.
enum SecretScan {
    Clean,
    Findings(usize),
    Unavailable(String),
}

async fn trufflehog_scan(ctx: &RunnerContext, repo_dir: &Path) -> anyhow::Result<SecretScan> {
    let mut cmd = Command::new(&ctx.trufflehog_bin);
    cmd.arg("filesystem")
        .arg("--json")
        .arg("--no-update")
        .arg(repo_dir);

    // A spawn failure (binary missing/not executable) is "unavailable" — the
    // documented non-blocking case. Everything past this point means the
    // process actually ran.
    let out = match run_cmd(cmd, Duration::from_secs(TRUFFLEHOG_TIMEOUT_SECS), &[]).await {
        Ok(out) => out,
        Err(e) => {
            return Ok(SecretScan::Unavailable(format!(
                "could not run trufflehog: {e}"
            )))
        }
    };
    if out.timed_out {
        anyhow::bail!("trufflehog timed out after {TRUFFLEHOG_TIMEOUT_SECS}s");
    }
    // Ran but exited non-zero (e.g. an unknown/renamed flag after a version
    // bump, or a mid-scan read error): its stdout is not an authoritative
    // clean result. Fail closed via the caller's `Err` arm.
    if !out.success {
        anyhow::bail!("trufflehog exited non-zero: {}", tail(&out.stderr, 500));
    }

    let verified = out
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| v.get("Verified").and_then(Value::as_bool) == Some(true))
        .count();
    if verified == 0 {
        Ok(SecretScan::Clean)
    } else {
        Ok(SecretScan::Findings(verified))
    }
}

// ---------------------------------------------------------------------------
// Internal control-plane API

async fn internal_post(ctx: &RunnerContext, spec: &JobSpec, action: &str) -> anyhow::Result<Value> {
    internal_post_body(ctx, spec, action, None).await
}

async fn internal_post_body(
    ctx: &RunnerContext,
    spec: &JobSpec,
    action: &str,
    body: Option<&Value>,
) -> anyhow::Result<Value> {
    let url = format!(
        "{}/api/internal/jobs/{}/{}",
        ctx.api_base_url, spec.job_id, action
    );
    let mut req = ctx.http.post(url).bearer_auth(&spec.job_token);
    if let Some(b) = body {
        req = req.json(b);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("internal api {}: {}", action, e.without_url()))?;
    let status = resp.status();
    if !status.is_success() {
        anyhow::bail!("internal api {} returned {}", action, status);
    }
    resp.json()
        .await
        .map_err(|e| anyhow::anyhow!("internal api {}: {}", action, e.without_url()))
}

/// Batch size for `wiki-urls`. Kept below the control plane's per-request
/// cap (`MAX_WIKI_PAGES_PER_REQUEST` = 500) so a large wiki is chunked
/// instead of hard-failing the whole upload after coral already ran.
const WIKI_URL_BATCH: usize = 400;
// Must stay under the control plane's per-request cap (500) or a full batch
// would be rejected; enforced at compile time.
const _: () = assert!(WIKI_URL_BATCH < 500);

async fn request_wiki_urls(
    ctx: &RunnerContext,
    spec: &JobSpec,
    slugs: &[String],
) -> anyhow::Result<std::collections::BTreeMap<String, String>> {
    let mut all = std::collections::BTreeMap::new();
    for batch in slugs.chunks(WIKI_URL_BATCH) {
        let value =
            internal_post_body(ctx, spec, "wiki-urls", Some(&json!({ "slugs": batch }))).await?;
        let urls = value
            .get("urls")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("wiki-urls response missing urls field"))?;
        let map: std::collections::BTreeMap<String, String> = serde_json::from_value(urls)?;
        all.extend(map);
    }
    Ok(all)
}

// ---------------------------------------------------------------------------
// Pre-signed URL transfers

async fn download(ctx: &RunnerContext, url: &str) -> anyhow::Result<Vec<u8>> {
    let resp = ctx
        .http
        .get(url)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("download: {}", e.without_url()))?;
    let status = resp.status();
    if !status.is_success() {
        anyhow::bail!("download failed with status {}", status);
    }
    Ok(resp
        .bytes()
        .await
        .map_err(|e| anyhow::anyhow!("download body: {}", e.without_url()))?
        .to_vec())
}

async fn upload_put(
    ctx: &RunnerContext,
    url: &str,
    body: Vec<u8>,
    content_type: &str,
) -> anyhow::Result<()> {
    let resp = ctx
        .http
        .put(url)
        .header(reqwest::header::CONTENT_TYPE, content_type)
        .body(body)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("upload: {}", e.without_url()))?;
    let status = resp.status();
    if !status.is_success() {
        anyhow::bail!("upload failed with status {}", status);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Wiki packaging

fn create_tar_zst(parent: &Path, dir_name: &str) -> anyhow::Result<Vec<u8>> {
    let encoder = zstd::stream::write::Encoder::new(Vec::new(), 3)?;
    let mut builder = tar::Builder::new(encoder);
    builder.append_dir_all(dir_name, parent.join(dir_name))?;
    let encoder = builder.into_inner()?;
    Ok(encoder.finish()?)
}

/// `tar::Archive::unpack` refuses entries that escape `dest`, so a
/// malicious tarball can't traverse out of the workdir.
fn extract_tar_zst(bytes: &[u8], dest: &Path) -> anyhow::Result<()> {
    let decoder = zstd::stream::read::Decoder::new(std::io::Cursor::new(bytes))?;
    let mut archive = tar::Archive::new(decoder);
    archive.unpack(dest)?;
    Ok(())
}

/// All `.md` files under the wiki dir as `(slug, path)`, slugified to the
/// shape the render endpoint accepts (`[a-z0-9-]+`). Nested paths join
/// with `-`. First slug wins on collision.
fn collect_pages(wiki_dir: &Path) -> anyhow::Result<Vec<(String, PathBuf)>> {
    let mut files = Vec::new();
    walk_md_files(wiki_dir, &mut files)?;
    files.sort();

    let mut seen = std::collections::BTreeSet::new();
    let mut pages = Vec::new();
    for path in files {
        match path.metadata() {
            Ok(m) if m.len() > MAX_PAGE_BYTES => {
                tracing::warn!(path = %path.display(), "wiki page exceeds size cap, skipping");
                continue;
            }
            _ => {}
        }
        let rel = path.strip_prefix(wiki_dir).expect("under wiki_dir");
        let Some(slug) = slugify(rel) else {
            tracing::warn!(path = %path.display(), "wiki page name does not slugify, skipping");
            continue;
        };
        if !seen.insert(slug.clone()) {
            tracing::warn!(slug, "duplicate wiki slug, keeping first");
            continue;
        }
        pages.push((slug, path));
    }
    Ok(pages)
}

fn walk_md_files(dir: &Path, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            walk_md_files(&path, out)?;
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("md"))
        {
            out.push(path);
        }
    }
    Ok(())
}

/// `guides/Auth Flow.md` → `guides-auth-flow`. Returns `None` when
/// nothing valid remains.
fn slugify(rel: &Path) -> Option<String> {
    let mut raw = String::new();
    for comp in rel.components() {
        let part = comp.as_os_str().to_string_lossy();
        if !raw.is_empty() {
            raw.push('-');
        }
        raw.push_str(&part);
    }
    let raw = raw
        .strip_suffix(".md")
        .or_else(|| raw.strip_suffix(".MD"))
        .unwrap_or(&raw);

    let mut slug = String::with_capacity(raw.len());
    let mut prev_dash = true; // suppress leading dash
    for c in raw.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            slug.push(c);
            prev_dash = false;
        } else if !prev_dash {
            slug.push('-');
            prev_dash = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() || slug.len() > 200 {
        return None;
    }
    Some(slug)
}

// ---------------------------------------------------------------------------
// Output parsing

fn parse_json_output(stdout: &str) -> Option<Value> {
    let trimmed = stdout.trim();
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        return Some(v);
    }
    // Tools that stream human-readable progress often emit JSON as the
    // last line.
    trimmed
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .and_then(|l| serde_json::from_str(l.trim()).ok())
}

async fn read_state_file(repo_dir: &Path) -> Option<Value> {
    let path = repo_dir.join(".coral").join(".bootstrap-state.json");
    let bytes = tokio::fs::read(&path).await.ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Depth-first search for the first numeric value under any of `keys`.
/// The coral state-file schema isn't pinned; tolerate nesting.
fn find_number(v: &Value, keys: &[&str]) -> Option<f64> {
    match v {
        Value::Object(map) => {
            for k in keys {
                if let Some(n) = map.get(*k).and_then(Value::as_f64) {
                    return Some(n);
                }
            }
            map.values().find_map(|child| find_number(child, keys))
        }
        Value::Array(items) => items.iter().find_map(|child| find_number(child, keys)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Failure + mock

fn failed(spec: &JobSpec, reason: &str, message: &str) -> JobResult {
    JobResult {
        job_id: spec.job_id,
        status: JobStatus::Failed,
        new_wiki_key: None,
        output: Value::Null,
        error: Some(message.to_string()),
        failure_reason: Some(reason.to_string()),
        cost_usd: None,
        input_tokens: None,
        output_tokens: None,
        duration_ms: 0, // stamped by `run`
    }
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
            "message": "Mock run — WORKER_MOCK_MODE is enabled"
        }),
        error: None,
        failure_reason: None,
        cost_usd: Some(0.0),
        input_tokens: Some(0),
        output_tokens: Some(0),
        duration_ms: 2000,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_flat_and_nested() {
        assert_eq!(
            slugify(Path::new("overview.md")).as_deref(),
            Some("overview")
        );
        assert_eq!(
            slugify(Path::new("guides/Auth Flow.md")).as_deref(),
            Some("guides-auth-flow")
        );
        assert_eq!(slugify(Path::new("API_v2.md")).as_deref(), Some("api-v2"));
        assert_eq!(slugify(Path::new("---.md")), None);
    }

    #[test]
    fn find_number_nested() {
        let v = json!({ "summary": { "totals": { "cost_usd": 1.25 } }, "input_tokens": 42 });
        assert_eq!(find_number(&v, COST_KEYS), Some(1.25));
        assert_eq!(find_number(&v, INPUT_TOKEN_KEYS), Some(42.0));
        assert_eq!(find_number(&v, &["missing"]), None);
    }

    #[test]
    fn parse_json_output_whole_and_last_line() {
        assert_eq!(
            parse_json_output(r#"{"ok":true}"#),
            Some(json!({"ok": true}))
        );
        assert_eq!(
            parse_json_output("progress 1/3\nprogress 2/3\n{\"done\":true}\n"),
            Some(json!({"done": true}))
        );
        assert_eq!(parse_json_output("plain text only"), None);
    }

    #[test]
    fn scrub_removes_secrets() {
        let s = "fatal: could not read from https://x:ghs_abc123@github.com";
        assert!(!scrub_secrets(s, &["ghs_abc123"]).contains("ghs_abc123"));
    }

    #[test]
    fn tarball_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let wiki = dir.path().join(".wiki");
        std::fs::create_dir_all(wiki.join("guides")).unwrap();
        std::fs::write(wiki.join("overview.md"), "# Overview").unwrap();
        std::fs::write(wiki.join("guides/setup.md"), "# Setup").unwrap();

        let bytes = create_tar_zst(dir.path(), ".wiki").unwrap();

        let out = tempfile::tempdir().unwrap();
        extract_tar_zst(&bytes, out.path()).unwrap();
        assert_eq!(
            std::fs::read_to_string(out.path().join(".wiki/overview.md")).unwrap(),
            "# Overview"
        );
        assert_eq!(
            std::fs::read_to_string(out.path().join(".wiki/guides/setup.md")).unwrap(),
            "# Setup"
        );
    }

    #[test]
    fn collect_pages_slugs_and_skips() {
        let dir = tempfile::tempdir().unwrap();
        let wiki = dir.path();
        std::fs::create_dir_all(wiki.join("guides")).unwrap();
        std::fs::write(wiki.join("overview.md"), "x").unwrap();
        std::fs::write(wiki.join("guides/Auth Flow.md"), "x").unwrap();
        std::fs::write(wiki.join("notes.txt"), "x").unwrap();

        let pages = collect_pages(wiki).unwrap();
        let slugs: Vec<&str> = pages.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(slugs, vec!["guides-auth-flow", "overview"]);
    }
}
