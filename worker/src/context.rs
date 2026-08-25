//! Runner configuration resolved once at startup.

use std::path::PathBuf;

#[derive(Clone)]
pub struct RunnerContext {
    /// HTTP client for internal-api calls and pre-signed URL transfers.
    pub http: reqwest::Client,
    /// Control-plane base URL (Railway internal), e.g.
    /// `http://api.railway.internal:8080`. Required unless `mock_mode`.
    pub api_base_url: String,
    /// Path or name of the `coral` binary (`CORAL_BIN`, default `coral`).
    pub coral_bin: String,
    /// Path or name of `trufflehog` (`TRUFFLEHOG_BIN`, default
    /// `trufflehog`). Missing binary downgrades to a warning — the scan
    /// is a gate, not a hard dependency, until GAP #68 is fully closed.
    pub trufflehog_bin: String,
    /// Parent directory for per-job workdirs (`WORKER_WORK_ROOT`,
    /// default the OS temp dir).
    pub work_root: PathBuf,
    /// Injected into the coral subprocess env only — never logged.
    pub anthropic_api_key: Option<String>,
    /// `WORKER_MOCK_MODE=true` restores the old fake-result behavior
    /// for environments without a coral binary.
    pub mock_mode: bool,
}

impl RunnerContext {
    pub fn from_env() -> anyhow::Result<Self> {
        let mock_mode = std::env::var("WORKER_MOCK_MODE")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        let api_base_url = match std::env::var("API_BASE_URL") {
            Ok(u) => u.trim_end_matches('/').to_string(),
            Err(_) if mock_mode => String::new(),
            Err(_) => anyhow::bail!(
                "required env var API_BASE_URL not set (or set WORKER_MOCK_MODE=true)"
            ),
        };

        let anthropic_api_key = std::env::var("ANTHROPIC_API_KEY").ok();
        if anthropic_api_key.is_none() && !mock_mode {
            tracing::warn!("ANTHROPIC_API_KEY not set — coral runs will fail unless tenants BYOK");
        }

        Ok(Self {
            http: reqwest::Client::builder()
                .user_agent("coral-saas-worker/0.1")
                .build()?,
            api_base_url,
            coral_bin: std::env::var("CORAL_BIN").unwrap_or_else(|_| "coral".into()),
            trufflehog_bin: std::env::var("TRUFFLEHOG_BIN").unwrap_or_else(|_| "trufflehog".into()),
            work_root: std::env::var("WORKER_WORK_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|_| std::env::temp_dir()),
            anthropic_api_key,
            mock_mode,
        })
    }
}
