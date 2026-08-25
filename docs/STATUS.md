# Coral-SAAS — Build Status

> Last updated by Claude session of 2026-08-25 (previous: 2026-05-25).

This document tracks what's been built, what compiles, what works, and what doesn't. Read alongside `SAAS-PLAN.md` (the target architecture) and `SAAS-PLAN-GAPS.md` (the 70 production gaps).

## Quick overview

```
Fase 0 — Setup            ✅ done
Fase 1 — Auth + tenant    ✅ done
Fase 2 — GitHub App       ✅ done (webhook handler + install callback)
Fase 3 — Job system       ✅ done in code — REAL coral subprocess pipeline, hermetically tested
Fase 4 — Wiki render      🟡 R2 client + page render + worker upload done; query SSE pending
Fase 5 — Polish + launch  🟡 frontend scaffold done; Stripe checkout + landing pending
```

`cargo check --workspace`, `cargo clippy --all-targets -D warnings`, and `cargo fmt --check` are clean. **18 tests pass** (`cargo test --workspace`): 8 api unit (wiki render, slug validation, job-token JWT), 6 worker unit (slugify, tarball roundtrip, JSON/cost parsing, secret scrubbing), 4 worker integration (hermetic full-pipeline runs: happy path, coral non-zero exit, timeout kill, missing-wiki failure).

The full path implemented in code (needs Railway + secrets to run live, see A1 in NEXT-SESSION.md):
**login → OAuth → personal tenant auto-created → install GitHub App → repos appear → Run bootstrap → api pre-signs R2 URLs + mints per-job JWT → worker clones via installation token (header auth) → trufflehog gate → coral subprocess with timeout → wiki tarball + per-page upload to R2 → job + repo status updated → wiki pages render.**

What's NOT yet verified end-to-end:
- A live run against real GitHub/R2/Railway (all code paths are exercised by the hermetic integration tests with a fake coral + local git remote + in-process R2 substitute; a real `coral bootstrap` against a real repo needs the A1 manual setup).
- The exact CLI contract of the real coral binary (`--wiki-root/--provider/--max-cost/--json` per SAAS-PLAN §9.2 comments) — verify on first live run.

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
- ✅ Enqueue: DB insert through RLS tx + RPUSH to Redis `coral:jobs` queue. **The jobs row id IS `spec.job_id`** — a 2026-08-25 fix; previously the row and the Redis spec had independently generated UUIDs, so the worker's claim-by-id never matched and every job sat in `queued` forever (the mock "end-to-end" path was actually broken).
- ✅ Bootstrap enqueue (`routes/jobs.rs`): atomic `bootstrap_status` gate (no duplicate concurrent bootstraps), pre-signed R2 PUT for the wiki tarball (TTL = timeout × 1.2) + GET for a prior wiki, per-job JWT minted with `WORKER_JWT_SECRET`.
- ✅ Internal worker API (`routes/internal_jobs.rs`), authenticated per-job JWT (HS256, aud=worker, sub=job_id, tenant cross-check, job must be `running`):
  - `POST /api/internal/jobs/:id/clone-token` → short-lived GitHub installation token (so it never sits in Redis).
  - `POST /api/internal/jobs/:id/wiki-urls` `{slugs}` → pre-signed PUT per wiki page (slug-validated, ≤500/request).
- ✅ **Real coral subprocess pipeline** (`worker/src/coral_runner.rs`): fresh workdir → shallow clone (installation token via `Authorization: basic` header, never in the URL; scrubbed from all captured output) → restore prior wiki tarball if provided → trufflehog gate (bootstrap; verified findings ⇒ `failure_reason=secrets_detected`, missing binary ⇒ warn+skip) → `coral <cmd> --wiki-root .wiki --provider anthropic_api --json [--max-cost N]` with per-kind timeout + kill-on-drop → parse stdout JSON (fallback `.coral/.bootstrap-state.json` for cost/tokens, tolerant nested search) → tar+zstd the `.wiki/` (pure Rust, no system tar) → upload tarball via spec PUT URL + each page via minted per-page URLs (nested paths slugified `guides/Auth Flow.md` → `guides-auth-flow`) → workdir cleanup always.
- ✅ Mock mode now env-driven: `WORKER_MOCK_MODE=true` (default **false**). Worker requires `API_BASE_URL` unless mocked.
- ✅ Classified failures in `JobResult.failure_reason`: `secrets_detected`, `timeout`, `coral_exit`, `clone_failed`, `no_wiki`, `unsupported_kind`, `invalid_args` (+ `worker_panic` for infra errors). Worker mirrors bootstrap outcome onto `repos.bootstrap_status` (`ready`/`failed`) + `wiki_s3_key` + `bootstrap_cost_usd`.
- ✅ Suicide-restart after `WORKER_MAX_JOBS` (default 100) — Railway auto-restarts clean (closes leak risk per §9.4).
- ✅ Coral binary vendored in `worker/Dockerfile`: release v0.41.0 (exists upstream, verified) pinned by sha256; **build fails** if fetch/checksum fails.
- ❌ Worker still writes job/repo outcomes directly to Postgres (SAAS-PLAN §9.2 / GAP #32) — the JWT internal API covers GitHub/R2 grants only. Moving the outcome writes behind it is the remaining piece of Track A5.
- ❌ Job cancellation flow (GAP #60).
- ❌ SSE endpoint for live job status — frontend polls instead (queries.ts `useJob` with refetchInterval).
- ⚠️ Timeout kills the direct child only; grandchildren spawned by coral could linger until container restart (fine on Railway one-job-per-replica; revisit with process groups if that changes).

### Wiki (`api/src/wiki/`, `api/src/r2/`, `api/src/routes/wiki.rs`)

- ✅ R2 client via `aws-sdk-s3` with endpoint override + path-style addressing.
- ✅ `get_object` / `put_object` / `presigned_get` / `presigned_put` helpers.
- ✅ Markdown render: `pulldown-cmark` (tables, footnotes, strikethrough, tasklists, smart punctuation) + `ammonia` sanitizer (allowlists `class` on code/pre for syntax hint preservation).
- ✅ Page route: `/api/tenants/:tenant_id/repos/:repo_id/wiki/:slug` with slug-regex guard (`[a-z0-9-]+`) against path traversal.
- ✅ Unit tests: heading extraction, script-tag stripping, code-class preservation, slug validation.
- ✅ Worker uploads both the `wiki.tar.zst` tarball (backup/portability, key stored in `repos.wiki_s3_key`) AND each page as `tenants/<t>/repos/<r>/wiki/<slug>.md` — exactly what this render endpoint reads. (Was the old "tarball extraction" gap — resolved worker-side per NEXT-SESSION A6 option 1.)
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

- ✅ 18 tests pass (`cargo test --workspace`):
  - api (8): wiki render ×3, slug validation, job-token JWT mint/verify ×4.
  - worker unit (6): slugify, tarball create/extract roundtrip, cost/token extraction, stdout JSON parsing, secret scrubbing, page collection.
  - worker integration (4, hermetic — local git remote + fake `coral` script + in-process axum standing in for the control plane and R2): happy-path bootstrap with uploads verified byte-for-byte, coral non-zero exit classified, timeout kill, no-wiki failure. Run on Windows and Linux alike (no Docker needed).
- ❌ No integration tests against real Postgres/RLS (require Docker).
- ❌ Zero E2E tests against the full deployed stack.

## CI status

`.github/workflows/ci.yml` runs:
- `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`
- `pnpm typecheck`, `pnpm lint`
- Docker build of api + worker images, then Trivy scan (CRITICAL/HIGH, `ignore-unfixed`)

**As of 2026-08-25** (first time ever): the Rust job and the container job are **green** —
both images build for real and pass the vulnerability gate. Getting there surfaced and
fixed a chain of latent Dockerfile bugs: builder image too old for the lockfile
(`rust:1.83` → `1.95`), coral release tarball nesting (`--strip-components`), stale
cargo fingerprints silently shipping a dummy `fn main(){}` api binary, missing
`COPY migrations` (compile-time `sqlx::migrate!`), bookworm→trixie runtime, and an
outdated trufflehog with 57 fixed-upstream CVEs (→ 3.97.1).

**Still failing (expected)**: the Next.js job — `pnpm install` needs the
`GITHUB_PACKAGES_TOKEN` secret in GitHub Actions repo settings (user-held, part of A1).
The separate "Deploy to Railway" workflow also fails until Railway is configured.

## What's next (in priority order)

1. **A1 — manual setup** (user-driven, ~1-2h): Railway project + Postgres/Redis add-ons, GitHub OAuth App + GitHub App, R2 bucket + keys, Stripe test mode, `GITHUB_PACKAGES_TOKEN`. Steps in NEXT-SESSION.md §A1. New env vars since then: `API_BASE_URL` (worker → api internal URL) and optionally `WORKER_MOCK_MODE`.
2. **Live smoke test**: deploy, connect a small real repo, run bootstrap, verify wiki renders. First live run validates the coral CLI contract (`--wiki-root/--provider/--max-cost/--json`) — adjust `coral_runner.rs` arg building if the real binary differs.
3. **Add query endpoint** with SSE for live LLM responses (B1 — runner already dispatches `JobKind::Query`).
4. **Add Stripe checkout endpoint** + frontend upgrade button (B3).
5. **Finish A5**: move worker's job/repo outcome writes behind the internal JWT API (grants half is done).
6. **Address remaining GAPs** in priority order from SAAS-PLAN-GAPS.md TOP 10.
