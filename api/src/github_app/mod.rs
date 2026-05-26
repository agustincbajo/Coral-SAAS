//! GitHub App integration — JWT signing for app auth, installation
//! token caching, webhook signature verification.
//!
//! Separate from `auth::github_oauth` which is for *user* login. This
//! module deals with the App acting on behalf of an installation.

pub mod installation_token;
pub mod jwt;
pub mod webhook;
