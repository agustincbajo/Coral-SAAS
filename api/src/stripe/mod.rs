//! Stripe integration — webhook verification + subscription sync.
//!
//! We never call Stripe from this codebase (yet) — payments come
//! through Stripe Checkout from the SPA, and the webhook is what
//! tells us about lifecycle changes.

pub mod webhook;
