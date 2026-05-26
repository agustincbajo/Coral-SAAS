# Coral-SAAS — Build Status

> Last updated by autonomous Claude session ending 2026-05-25.

This document tracks what's been built, what compiles, what works, and what doesn't. Read alongside `SAAS-PLAN.md` (the target architecture) and `SAAS-PLAN-GAPS.md` (the 70 production gaps).

## Quick overview

```
Fase 0 — Setup            ✅ done
Fase 1 — Auth + tenant    ✅ done
Fase 2 — GitHub App       ✅ done (webhook handler + install callback)
Fase 3 — Job system       🟡 wired end-to-end with MOCK coral subprocess
Fase 4 — Wiki render      🟡 R2 client + page render done; query SSE pending
Fase 5 — Polish + launch  🟡 frontend scaffold done; Stripe checkout + landing pending
```

`cargo check --workspace` is clean. 4 unit tests pass (`cargo test -p api --bin api`).

The first end-to-end path that works today (with secrets configured):
**login → OAuth → personal tenant auto-created → repos list (empty) → install GitHub App → install callback links it to tenant → repos appear → Run bootstrap → mock job runs → status updates to succeeded.**

What's NOT yet end-to-end:
- Bootstrap producing real wiki content (worker runs in mock-mode until the Coral binary is vendored in the worker Docker image — the path is laid out in `worker/Dockerfile`).
- Wiki page rendering against real R2 content (the route works, but there are no objects to fetch until a real bootstrap runs).

## What's implemented (by feature)

### Database (`api/src/db/`, `migrations/`)

- ✅ `migrations/0001_init.sql` — full schema: tenants, users, sessions, tenant_members, github_installations, repos, jobs, usage_ledger, tenant_secrets, audit_events, stripe_events. RLS policies on every tenant-scoped table.
- ✅ Postgres pool setup with bounded connections + idle timeout.
- ✅ `db::set_tenant(tx, tenant_id)` helper — caller-owned tx, `SET LOCAL app.tenant_id`. Compatible with pgbouncer transaction-pool mode (closes GAP #19).
- ✅ Models: Tenant, User, TenantMember, Session, GithubInstallation, Repo, Job. FromRow derives + idempotent upserts where applicable.
- ❌ No sqlx prepare metadata — queries are string-based at runtime. Reason: no Docker available to spin up Postgres at build time. Switch to `query_as!` macros + offline mode once Docker is in dev env.

### Auth (`api/src/auth/`, `api/src/routes/auth.rs`)

- ✅ GitHub OAuth (`read:user user:email` scope). Authorize URL builder, token exchange, profile fetch with `/user/emails` fallback for users hiding their primary email.
- ✅ Cookie-signed sessions with `SameSite=Strict; Secure; HttpOnly`. Session ID regenerated on every login (closes session-fixation GAP #31).
- ✅ CSRF — double-submit cookie + `X-CSRF-Token` header, constant-time compare (closes GAP #30 / SAAS-PLAN §8.4).
- ✅ AuthUser Axum extractor — pulls the session row from DB on every protected request. Returns 401 if missing/expired.
- ✅ Auto-create personal tenant on first OAuth callback. Slug derived from `github_login`, suffix-incremented on collision.
- ❌ No 2FA, no SSO, no audit on individual login events (writes are framework-level via tracing).

### GitHub App (`api/src/github_app/`, `api/src/routes/github_webhook.rs`, `github_install.rs`)

- ✅ App JWT signing (RS256, 9-min TTL with backdate for clock skew).
- ✅ Installation token cache in Redis with single-flight refresh (closes GAP #2 thundering herd).
- ✅ Webhook verification: HMAC-SHA256 with constant-time compare.
- ✅ Idempotency via Redis (`X-GitHub-Delivery`, 24h TTL).
- ✅ Event dispatch:
  - `installation.created` → logs (linkage happens via the redirect handler since the webhook doesn't carry tenant context).
  - `installation.deleted` → mark `disconnected_at` (30-day grace per §7.5).
  - `installation.suspend/unsuspend` → toggle `suspended_at`.
  - `installation_repositories.{added,removed}` → upsert/disconnect repos through RLS tx.
  - `repository.{renamed,edited,transferred,deleted}` → update full_name/default_branch/disconnect.
  - `push` → audit log + TODO enqueue ingest job.
  - `pull_request.*` → audit log only.
- ✅ Install callback (`/api/github/install/callback`): verifies AuthUser is owner/admin, fetches installation account info via app JWT, lists accessible repos via installation token, upserts everything, redirects to `/dashboard/repos`.
- ❌ Webhook secret rotation not implemented (GAP #1) — single secret, manual rotation only.
- ❌ Permission-upgrade re-consent flow (GAP #5) deferred to v2.

### Stripe (`api/src/stripe/`, `api/src/routes/stripe_webhook.rs`)

- ✅ Webhook signature verification (parses `t=...,v1=...` header, rejects >5min replay).
- ✅ Idempotency via the `stripe_events` table (INSERT ON CONFLICT DO NOTHING — strongly consistent).
- ✅ Handlers stubbed for: `checkout.session.completed` (links Stripe customer to tenant via `client_reference_id`), `customer.subscription.created/updated` (plan sync from price `lookup_key`), `customer.subscription.deleted` (downgrade to free), `invoice.payment_failed` (dunning placeholder).
- ❌ No checkout session creation endpoint yet — frontend can't initiate billing.
- ❌ No Stripe Tax / VAT setup (GAP #12).
- ❌ No dunning email flow (GAP #11).

### Jobs & Worker (`api/src/jobs/`, `worker/`)

- ✅ Job model: create → claim (atomic UPDATE WHERE status='queued') → complete.
- ✅ Enqueue: DB insert through RLS tx + RPUSH to Redis `coral:jobs` queue.
- ✅ Worker: BLPOP loop, atomic claim, mock subprocess (2s sleep + fake JobResult), persist outcome.
- ✅ Suicide-restart after `WORKER_MAX_JOBS` (default 100) — Railway auto-restarts clean (closes leak risk per §9.4).
- 🟡 Real coral subprocess: scaffolded in `worker/src/coral_runner.rs` behind `MOCK_MODE = true` const. Real path outline documented inline. Vendoring of the coral binary set up in `worker/Dockerfile` with a download-from-release pattern; pin a real version + SHA when shipping.
- 🟡 trufflehog secret scan (GAP #68): binary install scaffolded in worker/Dockerfile, not yet wired in `coral_runner.rs`.
- ❌ Worker → API callbacks via JWT (per SAAS-PLAN §9.2) — currently the worker writes directly to Postgres. MVP-acceptable on Railway internal network; needs JWT layer before scale-out to Fly Machines.
- ❌ Job cancellation flow (GAP #60).
- ❌ SSE endpoint for live job status — frontend polls instead (queries.ts `useJob` with refetchInterval).

### Wiki (`api/src/wiki/`, `api/src/r2/`, `api/src/routes/wiki.rs`)

- ✅ R2 client via `aws-sdk-s3` with endpoint override + path-style addressing.
- ✅ `get_object` / `put_object` / `presigned_get` / `presigned_put` helpers.
- ✅ Markdown render: `pulldown-cmark` (tables, footnotes, strikethrough, tasklists, smart punctuation) + `ammonia` sanitizer (allowlists `class` on code/pre for syntax hint preservation).
- ✅ Page route: `/api/tenants/:tenant_id/repos/:repo_id/wiki/:slug` with slug-regex guard (`[a-z0-9-]+`) against path traversal.
- ✅ Unit tests: heading extraction, script-tag stripping, code-class preservation, slug validation.
- ❌ Wiki tarball extraction on worker upload — currently we expect per-page `.md` objects in R2; worker would need to extract `wiki.tar.zst` and write each page. Trivial to add but not done.
- ❌ Wiki TF-IDF search (GAP #40) — only LLM queries via worker.
- ❌ Page navigation / sidebar of slugs.
- ❌ Backlinks display.

### Query (LLM)

- ❌ Not implemented yet. JobKind::Query exists in shared/ and the dispatch is wired in worker/, but no `/api/...query` endpoint nor SSE streaming.

### Frontend (`web/`)

- ✅ Next.js 15 + React 18 + TanStack Query + Zustand + Tailwind 4 + `@playsistemico/modo-bo-ui-lib` (wired via `.npmrc` + `transpilePackages`).
- ✅ Providers wrapper, api-client with credentials + CSRF, typed query hooks.
- ✅ Pages: `/login`, `/dashboard` (bounces), `/dashboard/repos`, `/dashboard/repos/[id]`, `/dashboard/repos/[id]/[slug]`.
- ✅ Sidebar with tenant selector + nav links. Topbar with avatar + sign-out.
- ❌ `pnpm install` / `pnpm typecheck` not yet verified from this side (requires `GITHUB_PACKAGES_TOKEN`). Patterns mirror em-dashboard so they should pass in CI.
- ❌ No real `modo-bo-ui-lib` components used yet — pages are plain Tailwind. Polish pass once typecheck runs.
- ❌ Empty-state UX, loading skeletons, toast notifications all minimal.

### Operational

- ✅ Audit log writer with `Actor` enum + `legal_retention` flag.
- ✅ Idempotency helper (Redis-backed) for webhooks.
- ✅ Error type with IntoResponse and 5xx scrubbing.
- ✅ Request-id middleware + structured tracing.
- ✅ `dotenvy` + fail-fast `Config::from_env()`.
- ✅ Migration runner in main.rs.
- ❌ No `/metrics` endpoint, no OpenTelemetry export wiring (lib added, not initialized).
- ❌ No health-with-deps endpoint (just `/healthz` returning 200).
- ❌ No graceful shutdown handler.

## Test coverage

- ✅ 4 unit tests pass: 3 in `wiki::render`, 1 in `routes::wiki`.
- ❌ Zero integration tests (require Postgres, blocked on Docker).
- ❌ Zero E2E tests against the full stack.

## CI status

`.github/workflows/ci.yml` runs:
- `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`
- `pnpm typecheck`, `pnpm lint`
- Trivy scan on api + worker images

**Will currently fail** because:
- `cargo clippy -D warnings` — there are dead_code warnings (the api crate has `#![allow(dead_code)]` so this might pass; verify).
- `pnpm install` will fail unless `GITHUB_PACKAGES_TOKEN` secret is set in GitHub Actions repo settings.

## What's next (in priority order)

1. **Configure secrets in Railway + GitHub Actions**:
   - `GITHUB_PACKAGES_TOKEN` (PAT with `read:packages`) — unblocks pnpm install.
   - All env vars from `.env.example` — unblock api runtime.
2. **Spin up Railway services + Postgres add-on + Redis add-on**.
3. **Create the GitHub App and OAuth App** on GitHub side; fill in IDs + private key + webhook secret.
4. **Vendor the Coral binary** in `worker/Dockerfile` — currently fetches from a release URL that may not exist yet; ship a real release or copy from local builds.
5. **Flip `MOCK_MODE = false`** in `coral_runner.rs` and implement the real subprocess path (clone → trufflehog → coral → upload).
6. **Add query endpoint** with SSE for live LLM responses.
7. **Add Stripe checkout endpoint** + frontend upgrade button.
8. **Address remaining GAPs** in priority order from SAAS-PLAN-GAPS.md TOP 10.
