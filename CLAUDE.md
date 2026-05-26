# CLAUDE.md — Coral-SAAS

> Repo-specific instructions for Claude Code working in this codebase.
> Source of truth for architecture: [docs/SAAS-PLAN.md](docs/SAAS-PLAN.md). Read it before making any structural changes.
> Source of truth for **current build state**: [docs/STATUS.md](docs/STATUS.md). Read it before assuming a feature is or isn't implemented.

## Project quick facts

- Coral-SAAS is a multi-tenant SaaS wrapper around the [Coral](https://github.com/agustincbajo/Coral) CLI.
- Three services: `api/` (Rust Axum), `worker/` (Rust queue consumer), `web/` (Next.js 15). Shared Rust types in `shared/`.
- Hosting: Railway (api + web + worker + Postgres + Redis), Cloudflare R2 for object storage.
- Frontend UI uses `@playsistemico/modo-bo-ui-lib` from GitHub Packages — `web/.npmrc` reads `GITHUB_PACKAGES_TOKEN` from env.

## Critical invariants

When making any change, double-check these:

### Tenant isolation
- Every `sqlx::query!` against a tenant-scoped table MUST filter by `tenant_id`.
- The control plane uses Postgres RLS as defense-in-depth. Before each request, middleware opens a transaction and runs `SET LOCAL app.tenant_id = $1`. **Never query a tenant-scoped table outside a transaction** — RLS silently degrades with pgbouncer transaction-mode if you do. See SAAS-PLAN.md §5.5.
- Redis cache keys MUST be prefixed with `t:<tenant_id>:`. Helper in `api/src/db/cache.rs` (when created).
- R2 object keys MUST start with `tenants/<id>/`. Worker pre-signs URLs via control plane — never direct credentials.

### Worker job lifecycle
- One Redis job = one child process. Worker parent never executes `coral` directly. See SAAS-PLAN.md §9.4.
- Every job MUST have a timeout (bootstrap 30min, ingest 10min, query 60s, implement 10min).
- Worker reports `JobResult` to control plane via short-lived JWT (per-job, not a static bearer). See §9.2.

### Secrets
- Never log: API keys (`ANTHROPIC_API_KEY`, `STRIPE_SECRET_KEY`), installation tokens, session tokens, JWTs, repo clone URLs (they contain installation tokens), tenant API keys from `tenant_secrets`.
- Tenant API keys are encrypted at rest with KMS (or equivalent). See §5.4.
- GitHub webhook secrets: validate HMAC before parsing payload. See §7.3.

### Auth & sessions
- Cookies: `SameSite=Strict; Secure; HttpOnly; Path=/`. Session regen on login (no fixation).
- CSRF: double-submit cookie on every state-changing POST/PUT/DELETE.
- See §8.4.

## Tooling expectations

### Rust
- `cargo fmt --all` before commit.
- `cargo clippy --all-targets --workspace -- -D warnings` must be green.
- `cargo test --workspace` must be green.
- Use `tracing` for logs, not `println!`. Structured fields.
- Errors: `thiserror` for typed errors, `anyhow` only at binary main.

### Next.js
- `pnpm typecheck` + `pnpm lint` must be green.
- Server components by default. `'use client'` only when interactivity is needed.
- Data fetching via TanStack Query for client; React Server Components for server-fetched data.
- State: Zustand for global UI state. NEVER persist sensitive data in Zustand.

### Migrations
- sqlx migrations in `migrations/`, numbered (e.g. `0001_init.sql`, `0002_add_audit_log.sql`).
- One concern per migration. Idempotent where possible.
- Big schema changes follow expand/contract — see §15 GAP #25.

## How to add a new feature

1. **Read SAAS-PLAN.md first** to understand where this feature fits in the phasing.
2. If the feature is in scope: open the relevant section of SAAS-PLAN.md, propose the change inline in the doc, and only then implement.
3. If the feature is OUT of scope (post-MVP): flag it. Don't sneak features in without plan update.
4. **Test the tenant isolation invariant** for any new endpoint or query: write a test that creates two tenants and verifies one can't see the other's data.

## Working with the Coral binary

The worker runs `coral` as a subprocess. Coral itself is at https://github.com/agustincbajo/Coral. **Never modify Coral logic from within Coral-SAAS** — if Coral needs a feature, send a PR upstream.

The worker Dockerfile copies the `coral` binary from a pinned upstream release. See `worker/Dockerfile` (look for `TODO: copy coral binary`).

## Reviewers' gap log

`docs/SAAS-PLAN-GAPS.md` lists 70 production gaps identified by reviewers. When implementing a feature, scan this file for related gaps and address them inline (or explicitly defer with a comment referencing the gap number).

## Tech-debt / known limitations to be aware of

- **`#![allow(dead_code)]` in api/src/main.rs**: scaffold-time concession; remove before launch and address each warning specifically.
- **No sqlx prepare metadata**: queries are runtime-typed (`sqlx::query_as::<_, T>(...)`). Switch to `query_as!` macros + `SQLX_OFFLINE=true` once Docker is available in dev to spin up a Postgres for `cargo sqlx prepare`.
- **Worker writes Postgres directly**: per SAAS-PLAN §9.2 the worker should call back to api via per-job JWTs. MVP shortcut for Railway internal-network trust; revisit before splitting compute (Fly Machines) or opening worker to non-trusted networks.
- **`coral_runner::MOCK_MODE = true`**: worker fakes its output. Flip to false + implement the real path once `worker/Dockerfile` is shipping a real coral binary (the download pattern is in the Dockerfile; pin a real release).

## Commit hygiene

- One logical feature per commit. Co-author trailer for Claude.
- Every commit message says what `cargo check --workspace` returned at HEAD.
- Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
