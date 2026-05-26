//! Wiki page fetch + markdown render + HTML sanitize.
//!
//! Layout in R2 per SAAS-PLAN §6.2:
//!   tenants/<tenant_id>/repos/<repo_id>/wiki/<slug>.md
//!
//! For the MVP wiki is one file per slug (no compression yet). The
//! worker's bootstrap path will tar-compress the whole `.wiki/` later;
//! we extract on first read and cache.

pub mod render;
