//! Runtime configuration loaded from env (with `dotenvy` for local dev).
//!
//! `Config::from_env()` is called once at startup and bailed if any
//! required field is missing — fail-fast is preferred over discovering
//! a missing `STRIPE_SECRET_KEY` at the moment Stripe is invoked.

use anyhow::{Context, Result};

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub redis_url: String,
    pub port: u16,
    pub session_secret: Vec<u8>,
    pub worker_jwt_secret: Vec<u8>,
    pub public_base_url: String,
    pub github_oauth: GithubOauthConfig,
    pub github_app: GithubAppConfig,
    pub stripe: StripeConfig,
    pub anthropic_api_key: Option<String>,
    pub r2: R2Config,
    pub resend_api_key: Option<String>,
}

#[derive(Clone, Debug)]
pub struct GithubOauthConfig {
    pub client_id: String,
    pub client_secret: String,
}

#[derive(Clone, Debug)]
pub struct GithubAppConfig {
    pub app_id: String,
    pub private_key_pem: String,
    pub webhook_secret: String,
}

#[derive(Clone, Debug)]
pub struct StripeConfig {
    pub secret_key: String,
    pub webhook_secret: String,
}

#[derive(Clone, Debug)]
pub struct R2Config {
    pub endpoint: String,
    pub bucket: String,
    pub access_key_id: String,
    pub secret_access_key: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let session_secret_hex = require_env("SESSION_SECRET")?;
        let session_secret = hex::decode(&session_secret_hex)
            .context("SESSION_SECRET must be hex-encoded")?;
        if session_secret.len() < 32 {
            anyhow::bail!("SESSION_SECRET must be at least 32 bytes (64 hex chars)");
        }

        let worker_jwt_secret_hex = require_env("WORKER_JWT_SECRET")?;
        let worker_jwt_secret = hex::decode(&worker_jwt_secret_hex)
            .context("WORKER_JWT_SECRET must be hex-encoded")?;
        if worker_jwt_secret.len() < 32 {
            anyhow::bail!("WORKER_JWT_SECRET must be at least 32 bytes (64 hex chars)");
        }

        Ok(Self {
            database_url: require_env("DATABASE_URL")?,
            redis_url: require_env("REDIS_URL")?,
            port: optional_env("PORT")?.unwrap_or(8080),
            session_secret,
            worker_jwt_secret,
            public_base_url: require_env("PUBLIC_BASE_URL")?,
            github_oauth: GithubOauthConfig {
                client_id: require_env("GITHUB_OAUTH_CLIENT_ID")?,
                client_secret: require_env("GITHUB_OAUTH_CLIENT_SECRET")?,
            },
            github_app: GithubAppConfig {
                app_id: require_env("GITHUB_APP_ID")?,
                private_key_pem: load_github_app_private_key()?,
                webhook_secret: require_env("GITHUB_WEBHOOK_SECRET")?,
            },
            stripe: StripeConfig {
                secret_key: require_env("STRIPE_SECRET_KEY")?,
                webhook_secret: require_env("STRIPE_WEBHOOK_SECRET")?,
            },
            anthropic_api_key: std::env::var("ANTHROPIC_API_KEY").ok(),
            r2: R2Config {
                endpoint: require_env("R2_ENDPOINT")?,
                bucket: require_env("R2_BUCKET")?,
                access_key_id: require_env("R2_ACCESS_KEY_ID")?,
                secret_access_key: require_env("R2_SECRET_ACCESS_KEY")?,
            },
            resend_api_key: std::env::var("RESEND_API_KEY").ok(),
        })
    }
}

fn require_env(key: &str) -> Result<String> {
    std::env::var(key).with_context(|| format!("required env var {} not set", key))
}

fn optional_env<T: std::str::FromStr>(key: &str) -> Result<Option<T>>
where
    T::Err: std::fmt::Display,
{
    match std::env::var(key) {
        Ok(s) => s
            .parse::<T>()
            .map(Some)
            .map_err(|e| anyhow::anyhow!("env var {} failed to parse: {}", key, e)),
        Err(_) => Ok(None),
    }
}

/// GitHub App private key is large multi-line PEM. Prefer a file path
/// pointed to by `GITHUB_APP_PRIVATE_KEY_PATH`, but accept the raw PEM
/// in `GITHUB_APP_PRIVATE_KEY` for ephemeral environments (Railway).
fn load_github_app_private_key() -> Result<String> {
    if let Ok(path) = std::env::var("GITHUB_APP_PRIVATE_KEY_PATH") {
        return std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read GitHub App private key from {}", path));
    }
    if let Ok(pem) = std::env::var("GITHUB_APP_PRIVATE_KEY") {
        return Ok(pem.replace("\\n", "\n"));
    }
    anyhow::bail!(
        "neither GITHUB_APP_PRIVATE_KEY_PATH nor GITHUB_APP_PRIVATE_KEY env var is set"
    )
}
