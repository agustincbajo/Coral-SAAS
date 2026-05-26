//! GitHub OAuth — user-facing login flow.
//!
//! Separate from the GitHub App (`github_app/`) which acts on behalf
//! of the installation. This OAuth App authenticates *humans*: we use
//! it solely to map the GitHub identity to our `users` table. We ask
//! for `read:user user:email` and nothing else.

use crate::{config::Config, error::ApiError};
use serde::Deserialize;
use url::Url;

const AUTHORIZE_URL: &str = "https://github.com/login/oauth/authorize";
const TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const USER_URL: &str = "https://api.github.com/user";
const EMAILS_URL: &str = "https://api.github.com/user/emails";

const SCOPE: &str = "read:user user:email";

#[derive(Debug, Deserialize)]
pub struct GithubUser {
    pub id: i64,
    pub login: String,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GithubEmail {
    email: String,
    primary: bool,
    verified: bool,
}

#[derive(Debug, Deserialize)]
struct OAuthTokenResponse {
    access_token: String,
    // GitHub returns scope + token_type but we don't use them.
}

/// Build the URL we redirect users to in step 1 of OAuth.
/// `state` is a random nonce we stash in a short-lived cookie to defend
/// against CSRF on the callback.
pub fn authorize_url(config: &Config, state: &str) -> String {
    let mut url = Url::parse(AUTHORIZE_URL).expect("authorize url is static");
    url.query_pairs_mut()
        .append_pair("client_id", &config.github_oauth.client_id)
        .append_pair("redirect_uri", &callback_url(config))
        .append_pair("scope", SCOPE)
        .append_pair("state", state)
        .append_pair("allow_signup", "true");
    url.to_string()
}

pub fn callback_url(config: &Config) -> String {
    format!("{}/auth/github/callback", config.public_base_url.trim_end_matches('/'))
}

/// Step 2: exchange the temporary `code` for an access token.
pub async fn exchange_code(
    http: &reqwest::Client,
    config: &Config,
    code: &str,
) -> Result<String, ApiError> {
    let res = http
        .post(TOKEN_URL)
        .header("Accept", "application/json")
        .header("User-Agent", "coral-saas")
        .form(&[
            ("client_id", config.github_oauth.client_id.as_str()),
            ("client_secret", config.github_oauth.client_secret.as_str()),
            ("code", code),
            ("redirect_uri", &callback_url(config)),
        ])
        .send()
        .await?
        .error_for_status()?
        .json::<OAuthTokenResponse>()
        .await?;

    Ok(res.access_token)
}

/// Step 3: fetch the user's GitHub profile so we can upsert into `users`.
pub async fn fetch_user(
    http: &reqwest::Client,
    access_token: &str,
) -> Result<GithubUser, ApiError> {
    let user: GithubUser = http
        .get(USER_URL)
        .bearer_auth(access_token)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "coral-saas")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    // If the public profile email is null (user kept it private), pull
    // the verified primary email from /user/emails.
    if user.email.is_some() {
        return Ok(user);
    }

    let emails: Vec<GithubEmail> = http
        .get(EMAILS_URL)
        .bearer_auth(access_token)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "coral-saas")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let primary = emails
        .into_iter()
        .find(|e| e.primary && e.verified)
        .map(|e| e.email);

    Ok(GithubUser {
        email: primary,
        ..user
    })
}
