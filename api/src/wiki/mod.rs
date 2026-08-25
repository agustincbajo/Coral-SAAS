//! Wiki page fetch + markdown render + HTML sanitize.
//!
//! Layout in R2 per SAAS-PLAN §6.2:
//!   tenants/<tenant_id>/repos/<repo_id>/wiki/<slug>.md
//!
//! For the MVP wiki is one file per slug (no compression yet). The
//! worker's bootstrap path will tar-compress the whole `.wiki/` later;
//! we extract on first read and cache.

pub mod render;

/// Allowed wiki slug shape: `[a-z0-9-]+`, ≤200 chars. Coral's own SCHEMA
/// uses kebab-case slugs; we belt-and-suspenders anywhere a slug becomes
/// part of an R2 key so path traversal never reaches storage.
pub fn is_safe_slug(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 200
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}
