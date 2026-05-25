# Coral SaaS — Gap Analysis (Reviewer Output)

**Source**: Análisis externo realizado por agente reviewer dedicado contra `docs/SAAS-PLAN.md` v0.1.
**Fecha**: 2026-05-24
**Total**: 70 gaps identificados, severidad LOW/MEDIUM/HIGH.

> **Cómo leer este documento**: cada gap tiene cita a fuente externa (docs, RFC, estándar) o marca `OPINION` cuando es juicio del reviewer. Severidad refleja: HIGH = bloquea launch, MEDIUM = bite dentro de 3 meses, LOW = nice-to-have. La Top-10 al final del documento sintetiza los más críticos por probabilidad×impacto.

---

## Category A — GitHub App

### GAP 1: Webhook secret rotation
- **Plan section**: §7.3 "Webhook receiver"
- **Missing**: Plan describes HMAC validation but never addresses how the webhook secret rotates. GitHub allows only ONE secret per app at a time, so rotation requires careful coordination (dual-validate during cutover, or schedule downtime).
- **Severity**: MEDIUM
- **Source**: GitHub Webhooks docs — securing webhooks (docs.github.com/en/webhooks/using-webhooks/validating-webhook-deliveries)
- **Why it matters**: Compromised webhook secret = attacker forges arbitrary GitHub events into your queue. No rotation = stuck forever.

### GAP 2: Installation token cache race conditions
- **Plan section**: §7.2.3 "Cacheo de installation tokens"
- **Missing**: TTL of 50min for a 1h token is fine, but plan doesn't address concurrent refresh — if two workers hit Redis miss simultaneously, both call `/access_tokens`. GitHub rate-limits token endpoints separately (5000 token requests/hour per app). No mutex/single-flight pattern is described.
- **Severity**: MEDIUM
- **Source**: GitHub Apps — generating installation access tokens (docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-an-installation-access-token-for-a-github-app)
- **Why it matters**: At scale, thundering herd on token refresh = wasted API budget and potential 429s from GitHub.

### GAP 3: `installation.deleted` event — data deletion policy
- **Plan section**: §7.1 events list (mentions `installation` but not specific actions)
- **Missing**: When user uninstalls the app, what happens to the wiki in S3? Postgres rows? Grace period? Plan does not specify deletion cascade or retention. GDPR Art. 17 (right to erasure) intersects here.
- **Severity**: HIGH
- **Source**: GitHub Apps — handling installation events (docs.github.com/en/webhooks/webhook-events-and-payloads#installation)
- **Why it matters**: Uninstall is the user's de facto "delete my data" signal. Silently keeping their wiki = ToS/GDPR violation.

### GAP 4: Suspended installations
- **Plan section**: §7 GitHub App integration
- **Missing**: GitHub allows org admins to *suspend* an installation (different from delete). Plan does not describe how the worker handles `installation_token` requests against a suspended install (they return 403). UX during suspension is undefined.
- **Severity**: MEDIUM
- **Source**: GitHub Apps — suspending installations (docs.github.com/en/apps/maintaining-github-apps/suspending-a-github-app-installation)
- **Why it matters**: Suspended state will cause cryptic job failures with no user-facing explanation.

### GAP 5: Permission upgrade re-consent
- **Plan section**: §7.1 "Para v2 (implement loop con PRs): contents:write, pull_requests:write"
- **Missing**: Plan acknowledges v2 needs more permissions but doesn't describe the *user re-consent flow*. Adding a permission to a GitHub App requires every existing installation to accept the new permissions; until they do, the app cannot use them. No migration plan.
- **Severity**: HIGH (for v2 launch)
- **Source**: GitHub Apps — editing a GitHub App's permissions (docs.github.com/en/apps/maintaining-github-apps/editing-a-github-apps-permissions)
- **Why it matters**: On v2 launch, 100% of users will see broken implement loop until each one manually re-approves.

### GAP 6: GitHub App rate limits — no capping
- **Plan section**: §7.4 says "5000/h por instalación" but §9.3 quotas only talk about jobs, not GitHub API calls
- **Missing**: A single bootstrap of a large repo with many files can blow through 5000 calls (clone is not REST-limited, but PR comments, listing branches, etc. are). Plan doesn't track per-installation GitHub API budget.
- **Severity**: MEDIUM
- **Source**: GitHub REST API — rate limits for GitHub Apps (docs.github.com/en/rest/overview/resources-in-the-rest-api#rate-limits-for-github-apps)
- **Why it matters**: An aggressive user can rate-limit themselves and not understand why.

### GAP 7: Repo transfers / renames / deletions
- **Plan section**: §5.2 `repos` table has `github_repo_id BIGINT UNIQUE` and `full_name TEXT`
- **Missing**: Plan correctly keys on `github_repo_id` (stable across rename) but doesn't describe the webhook handler for the `repository` event (`renamed`, `transferred`, `deleted`). `full_name` will go stale; transferred repos may move outside the installation's scope.
- **Severity**: MEDIUM
- **Source**: GitHub webhooks — repository event (docs.github.com/en/webhooks/webhook-events-and-payloads#repository)
- **Why it matters**: Bad UX when the wiki shows the old name; broken clones when the new owner hasn't installed the app.

### GAP 8: Dual install friction (App + OAuth)
- **Plan section**: §8.1 "GitHub App para acceso a repos, OAuth App para login"
- **Missing**: Plan recommends separation but doesn't quantify friction. Empirically, dual-install (OAuth + App) costs significant funnel. Plan doesn't measure or A/B this.
- **Severity**: MEDIUM
- **Source**: OPINION (but supported by the GitHub App docs' explicit recommendation to use "Sign in with GitHub" via the App itself: docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-a-user-access-token-for-a-github-app)
- **Why it matters**: GitHub Apps can act on behalf of users via user access tokens — a single install can do both login and repo access.

---

## Category B — Billing & legal

### GAP 9: Stripe webhook handling absent
- **Plan section**: §11 Pricing (no §11.x for webhooks)
- **Missing**: Plan mentions "Stripe Checkout + Customer Portal" but never describes Stripe webhook handling: `invoice.payment_failed`, `customer.subscription.deleted`, `customer.subscription.updated`. Without these, subscription state in `tenants.plan` drifts from reality at Stripe.
- **Severity**: HIGH
- **Source**: Stripe webhooks docs (stripe.com/docs/webhooks) and subscription lifecycle (stripe.com/docs/billing/subscriptions/overview)
- **Why it matters**: A user who cancels at Stripe still has Pro features. A failed payment doesn't trigger downgrade. Revenue leaks and unfair access result.

### GAP 10: Stripe webhook idempotency
- **Plan section**: same as above
- **Missing**: Plan describes idempotency for GitHub webhooks but not Stripe. Stripe explicitly says webhooks can be redelivered and require `event.id` dedup.
- **Severity**: MEDIUM
- **Source**: Stripe webhooks best practices (stripe.com/docs/webhooks#best-practices)
- **Why it matters**: Double-billing or double-applied credits.

### GAP 11: Dunning workflow
- **Plan section**: §11
- **Missing**: When a Pro user's card fails, what happens? Stripe Smart Retries? Email cadence? Plan grace period before downgrade?
- **Severity**: MEDIUM
- **Source**: Stripe dunning (stripe.com/docs/billing/revenue-recovery)
- **Why it matters**: Revenue recovery is typically 30-40% of MRR for SaaS — leaving it on default = churn.

### GAP 12: VAT/sales tax
- **Plan section**: §11
- **Missing**: Stripe Tax must be explicitly enabled and the customer's tax address collected. Plan doesn't mention tax at all. EU/UK VAT MOSS, US sales tax (post-Wayfair) are real.
- **Severity**: HIGH (for EU/UK customers)
- **Source**: Stripe Tax (stripe.com/tax)
- **Why it matters**: Selling to EU without VAT registration = tax liability. Each EU country has thresholds.

### GAP 13: Refund policy
- **Plan section**: nowhere
- **Missing**: No refund policy specified. Stripe and consumer-protection laws (e.g., EU 14-day right of withdrawal for B2C) require clarity.
- **Severity**: MEDIUM
- **Source**: EU Consumer Rights Directive 2011/83/EU
- **Why it matters**: B2C in EU without right-of-withdrawal language = legal exposure.

### GAP 14: Free tier abuse via N-tenant pattern
- **Plan section**: §13.1 "Rate limit por github_id"
- **Missing**: Plan rate-limits by github_id but doesn't prevent: one user creates N tenants (each rate-limited separately), or creates N GitHub accounts. Plan does not describe device fingerprinting, email verification cadence, or payment-card-on-file gating.
- **Severity**: MEDIUM
- **Source**: OPINION (well-documented anti-abuse pattern; see Stripe Radar docs)
- **Why it matters**: 100 queries/mo free × N tenants = your $500/mo bank-rolled LLM budget evaporates.

### GAP 15: ToS / Privacy Policy / DPA — no detail
- **Plan section**: §15 Fase 5 ("Privacy policy + Terms of Service" as a checkbox)
- **Missing**: No mention of: required ToS clauses (Anthropic AUP pass-through, IP ownership of generated wiki content, indemnification), DPA template for B2B, sub-processor list with notification cadence.
- **Severity**: HIGH (blocking for any enterprise sale; medium for B2C launch)
- **Source**: Anthropic Usage Policies (anthropic.com/legal/usage-policy) requires passing certain restrictions downstream; GDPR Art. 28 requires DPA between controller and processor.
- **Why it matters**: One enterprise prospect's legal team kills the deal in week 1.

### GAP 16: GDPR right-to-delete cascade
- **Plan section**: §13.2 "delete-my-data endpoint" (one-liner)
- **Missing**: Plan does not describe cascade: tenant DB rows, S3 wiki (R2 doesn't have built-in versioned-object-pruning policies parallel to S3), worker logs, Postgres backups (these retain deleted data!), Stripe customer record, observability traces.
- **Severity**: HIGH
- **Source**: GDPR Art. 17 (right to erasure) — text at gdpr-info.eu/art-17-gdpr/
- **Why it matters**: "Delete my data" that leaves data in 7-day rolling backups + S3 versions = not compliant.

### GAP 17: DMCA process
- **Plan section**: nowhere
- **Missing**: If the wiki contains scraped/copyrighted content (e.g., user runs Coral on a repo that vendors copyrighted code, or the implement loop generates infringing content), there's no DMCA designated agent, notice procedure, or counter-notice flow.
- **Severity**: MEDIUM
- **Source**: 17 U.S.C. § 512 (DMCA safe harbor) — requires registered agent ($6 for 3 years at copyright.gov)
- **Why it matters**: No DMCA agent = lose safe harbor for user-generated content.

### GAP 18: Anthropic AUP pass-through
- **Plan section**: §10.1 BYOK
- **Missing**: Even with BYOK, Coral SaaS facilitates the LLM use. Anthropic's Commercial Terms (or Usage Policy) imposes obligations on resellers/integrators. Plan doesn't address what the ToS must contain to pass these through to end users (e.g., no malware generation, no high-stakes decisions without human review).
- **Severity**: MEDIUM
- **Source**: Anthropic Usage Policy (anthropic.com/legal/usage-policy)
- **Why it matters**: If a user uses Coral SaaS to violate Anthropic's terms, your account can be terminated.

---

## Category C — Multi-tenancy & isolation

### GAP 19: RLS + pgbouncer transaction mode incompatibility
- **Plan section**: §5.5 "antes de cada request, control plane hace `SET LOCAL app.tenant_id`"
- **Missing**: `SET LOCAL` only persists for the duration of a transaction. In pgbouncer transaction-pool mode (the default for SaaS), connections are recycled per-transaction; if any code path forgets to wrap in a transaction, the `SET LOCAL` leaks or evaporates. Plan does not address pool mode constraint.
- **Severity**: HIGH
- **Source**: pgbouncer docs on pool modes (pgbouncer.org/usage.html) + Postgres `SET LOCAL` docs (postgresql.org/docs/current/sql-set.html)
- **Why it matters**: A bug here = cross-tenant data leak with RLS silently inactive.

### GAP 20: Per-tenant S3 IAM scoping not specified
- **Plan section**: §6.1 "IAM policy del worker scoped al prefix"
- **Missing**: Plan says "IAM scoped to prefix" but doesn't describe HOW. Options: per-tenant IAM role (AWS STS AssumeRole), per-tenant pre-signed URL minted by control plane, or single role with bucket policy. R2 doesn't support AWS STS at all (only API tokens).
- **Severity**: HIGH
- **Source**: Cloudflare R2 docs on tokens (developers.cloudflare.com/r2/api/s3/tokens/)
- **Why it matters**: If R2 was chosen but plan assumes STS-style per-tenant scoping, the implementation will not work as described.

### GAP 21: Cross-tenant cache leakage
- **Plan section**: §3.1 mentions CDN cache; §7.2 mentions Redis cache
- **Missing**: Plan does not describe cache key namespacing (must include tenant_id) or CDN cache rules that prevent tenant A's wiki page from being served to tenant B via shared edge cache.
- **Severity**: HIGH
- **Source**: OPINION (universal SaaS principle, no single citation)
- **Why it matters**: Cache poisoning or stale-after-revocation = leaks private wiki to anyone with the URL pattern.

### GAP 22: Anthropic API rate limit error propagation
- **Plan section**: §17.1 mentions "Anthropic 5xx burst" in observability
- **Missing**: When BYOK user hits their tier rate limit (Anthropic returns 429), the worker fails. Plan doesn't specify UX: retry, surface to user, or backoff?
- **Severity**: LOW
- **Source**: Anthropic API rate limits (docs.anthropic.com/en/api/rate-limits)
- **Why it matters**: Mysterious "job failed" with no explanation = support ticket flood.

---

## Category D — Operational

### GAP 23: Postgres backup strategy
- **Plan section**: §12.1 "Postgres: Railway/Supabase/Neon"
- **Missing**: Plan trusts the provider's default backups but doesn't specify: RPO (how much data can we lose?), retention period, restoration testing cadence, point-in-time recovery? Cross-region replicated?
- **Severity**: HIGH
- **Source**: OPINION (Railway docs default is 7d retention, Neon offers PITR — but plan doesn't pick or test)
- **Why it matters**: First DB corruption incident = "we restored from yesterday, lost a day of usage data and 6h of billing." Or worse.

### GAP 24: Disaster recovery RTO/RPO
- **Plan section**: §13.2 "Lo que NO necesita el MVP"
- **Missing**: No RTO (recovery time) or RPO (recovery point) targets, even informally.
- **Severity**: MEDIUM
- **Why it matters**: "How long can we be down?" should be a number, not a feeling.

### GAP 25: Zero-downtime DB migrations
- **Plan section**: §15 phases mention CI but not deploy
- **Missing**: Plan uses sqlx (compile-time checked) but doesn't describe migration approach: blue-green schema? Expand-contract? Lockstep deploys? Big migrations on a 100GB Postgres table will lock tables.
- **Severity**: MEDIUM
- **Source**: OPINION (well-known SaaS pattern: e.g., the GitLab "expand-contract" migration pattern)
- **Why it matters**: First `ALTER TABLE jobs ADD COLUMN ...` past 10GB = production lock-up.

### GAP 26: Deploy strategy
- **Plan section**: §4 mentions "GitHub Actions → Railway/Fly deploy"
- **Missing**: Not specified: blue-green, canary, or rolling? Railway and Fly each have specific patterns; plan doesn't pick one. No rollback procedure.
- **Severity**: MEDIUM
- **Why it matters**: A bad deploy without rollback procedure = extended outage.

### GAP 27: On-call & incident response
- **Plan section**: §14 mentions "Alerts: PagerDuty"
- **Missing**: Plan is a solo project initially. Who's on-call at 3am? What's the escalation? Backup contact? Incident runbook?
- **Severity**: MEDIUM (becomes HIGH when paying customers arrive)
- **Why it matters**: Solo founders burn out fast without on-call automation.

### GAP 28: Status page existence
- **Plan section**: §14 says "Statuspage.io (free)"
- **Missing**: Statuspage.io's free tier was discontinued. Atlassian Statuspage starts at $29/mo for basic. Free alternatives include statusgator, instatus (free tier), uptime-kuma (self-hosted). Plan's cost line item is wrong.
- **Severity**: LOW (cost estimate inaccuracy)
- **Source**: Atlassian Statuspage pricing (atlassian.com/software/statuspage/pricing)
- **Why it matters**: Surprise line-item; nothing critical but undermines plan accuracy.

### GAP 29: SLO/SLA for paid tiers
- **Plan section**: §11.1 lists features per tier but no uptime/latency commitments
- **Missing**: Pro at $19/mo, Team at $99/mo — no SLA. Enterprises will ask for 99.9% uptime SLA at minimum.
- **Severity**: MEDIUM
- **Why it matters**: First enterprise prospect: "what's your SLA?" → no answer = no deal.

---

## Category E — Auth/Security

### GAP 30: CSRF on state-changing endpoints
- **Plan section**: §8 Auth
- **Missing**: Cookie-based sessions are vulnerable to CSRF. Plan doesn't mention SameSite=Lax/Strict, double-submit cookie, or origin validation. POST `/api/query` could be CSRF'd from another origin via stored auth cookie.
- **Severity**: HIGH
- **Source**: OWASP CSRF Prevention Cheat Sheet (cheatsheetseries.owasp.org/cheatsheets/Cross-Site_Request_Forgery_Prevention_Cheat_Sheet.html)
- **Why it matters**: An attacker's site can drain a victim's LLM budget by forging queries.

### GAP 31: Session fixation
- **Plan section**: §8.1
- **Missing**: Plan doesn't describe regenerating session ID on login. Without rotation, a pre-login session ID survives login = session fixation.
- **Severity**: MEDIUM
- **Source**: OWASP Session Management Cheat Sheet (cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html)
- **Why it matters**: Classic auth flaw; trivial to prevent if explicit.

### GAP 32: Worker → control-plane bearer token rotation
- **Plan section**: §9.2 entrypoint uses `${WORKER_TOKEN}`
- **Missing**: This is a long-lived secret embedded in the worker image (or env). Plan doesn't describe: rotation cadence, scope (one token for all workers? per-tenant?), revocation procedure.
- **Severity**: HIGH
- **Source**: OPINION (NIST SP 800-63B recommends periodic rotation of long-lived secrets)
- **Why it matters**: One image leak = full control-plane API access.

### GAP 33: KMS key rotation for tenant secrets
- **Plan section**: §5.4 `tenant_secrets` has `key_version INT`
- **Missing**: Field exists but plan doesn't describe key rotation procedure: cadence, re-encryption job, rollback if rotation fails.
- **Severity**: MEDIUM
- **Source**: NIST SP 800-57 Part 1
- **Why it matters**: Compliance frameworks (SOC2, PCI) require documented rotation policy.

### GAP 34: BYOK key validation cadence
- **Plan section**: §10.1 "Detectar si la key es válida con un ping"
- **Missing**: Plan validates at submission, but Anthropic API keys can be revoked at any time. Plan doesn't describe periodic re-validation or how a worker discovers an expired key (silent 401, retried forever?).
- **Severity**: LOW
- **Why it matters**: Tenant's expired key = mysterious permanent job failures.

### GAP 35: Audit log absent at MVP
- **Plan section**: §13.2 "post-PMF"
- **Missing**: First enterprise prospect requirement: "show me who accessed the wiki when?" Plan defers all audit. For a multi-tenant SaaS with shared admin access, this is a major sales blocker post-PMF, and a forensics hole at MVP.
- **Severity**: HIGH
- **Source**: SOC 2 Common Criteria CC7.2 (aicpa.org)
- **Why it matters**: After any security incident, the first question is "what was accessed?" — no audit log = unanswerable.

### GAP 36: Worker container hardening
- **Plan section**: §9.2 Dockerfile
- **Missing**: Container runs as root (no `USER` directive). No seccomp profile, no read-only root FS, no `--cap-drop=all`. The `--depth=1` git clone runs in a writable rootFS.
- **Severity**: HIGH
- **Source**: NIST SP 800-190 Container Security + CIS Docker Benchmark
- **Why it matters**: If the implement loop generates malicious code (or a malicious repo poisons the worker), root in container = much easier escape.

### GAP 37: Container image scanning
- **Plan section**: §15 Fase 0 mentions `cargo deny` (Rust supply chain)
- **Missing**: Worker base is `debian:bookworm-slim`. No Trivy/Grype/Snyk scan in CI. Plan only covers Rust deps.
- **Severity**: MEDIUM
- **Source**: OWASP Container Top 10 + CIS Docker Benchmark item 4.4
- **Why it matters**: Unpatched debian CVEs in production worker image.

### GAP 38: Supply chain — base image pinning
- **Plan section**: §9.2 `FROM rust:1.83-slim` and `FROM debian:bookworm-slim`
- **Missing**: Tags are mutable. No SHA digest pinning. A compromised upstream tag = compromised build.
- **Severity**: MEDIUM
- **Source**: SLSA Supply Chain Levels (slsa.dev/spec/v1.0/levels)
- **Why it matters**: This is exactly the SolarWinds-style attack vector for container builds.

### GAP 39: Egress allowlist — implementation
- **Plan section**: §6.1 "egress restrictivo (allowlist)"
- **Missing**: Plan states the requirement but doesn't describe HOW on Fly Machines. Fly's egress controls are limited (no built-in firewall for outbound). Likely requires per-image iptables rules or a sidecar proxy — neither mentioned.
- **Severity**: MEDIUM
- **Source**: Fly.io networking docs (fly.io/docs/networking/)
- **Why it matters**: The egress allowlist is a major isolation claim, and plan doesn't actually deliver it.

---

## Category F — Product gaps

### GAP 40: Wiki search/discovery
- **Plan section**: §15 Fase 4 mentions navigation but not search
- **Missing**: Plan describes "queries" (LLM-based) but not direct search. A user with a known concept who just wants to find the page should not pay LLM cost. Coral has TF-IDF search; plan doesn't expose it.
- **Severity**: MEDIUM
- **Why it matters**: Users will quickly tire of paying for every find action.

### GAP 41: Wiki editing UX
- **Plan section**: §1.2 "No-goal: Editor colaborativo..."
- **Missing**: "From the UI" is mentioned but not designed. Lock semantics, conflict resolution when wiki auto-updates from new commits while user edits, persistence path (PR to repo? direct to S3?).
- **Severity**: MEDIUM
- **Why it matters**: Editing is implied as a feature but undefined.

### GAP 42: Staleness communication
- **Plan section**: nowhere
- **Missing**: When the source repo has new commits but wiki hasn't re-ingested, the wiki is stale. Plan doesn't describe how this is surfaced to the user (badge, banner, last-updated timestamp).
- **Severity**: MEDIUM
- **Why it matters**: Stale wiki users won't trust the tool.

### GAP 43: Default branch change handling
- **Plan section**: §5.2 `repos.default_branch` is stored
- **Missing**: When a repo's default branch changes (e.g., `master` → `main`), plan doesn't describe re-ingest, redirect, or detection.
- **Severity**: LOW
- **Source**: GitHub repository webhook (docs.github.com/en/webhooks/webhook-events-and-payloads#repository) — `edited` action with `default_branch`
- **Why it matters**: Wiki becomes silently outdated.

### GAP 44: Monorepo modeling
- **Plan section**: nowhere
- **Missing**: Plan treats one GitHub repo = one wiki. Monorepos (e.g., Google-style) have multiple sub-projects. Pricing tier "5 repos" is undefined for a monorepo with 50 subprojects.
- **Severity**: MEDIUM
- **Why it matters**: Real-world enterprise codebases are monorepos.

### GAP 45: Language coverage expectations
- **Plan section**: nowhere
- **Missing**: Coral has parsers for some languages but not all. Plan doesn't communicate this to the user during onboarding. Onboarding a Kotlin or Swift repo = silent quality cliff.
- **Severity**: MEDIUM
- **Why it matters**: Bad WOM ("Coral doesn't work for my Swift project").

---

## Category G — Cost model risks

### GAP 46: Free tier abuse cost projection
- **Plan section**: §11.2 "asumiendo 20% conversión"
- **Missing**: Math assumes no abuse. A single bad actor with 100 fake accounts = $300-500/mo in LLM at the BYOK 100-query/mo bonus. No abuse model.
- **Severity**: HIGH
- **Why it matters**: Free tier abuse can easily bankrupt a small SaaS before paying users arrive.

### GAP 47: Worker cold start cost not modeled
- **Plan section**: §16 "$300/mo for workers" without math
- **Missing**: Fly Machines start in ~2s but the full job startup adds: pulling Docker image (5-30s), git shallow clone (1-10s), S3 download of wiki (1-5s). For 60s queries, cold start is 25-50% of cost. No image-pull-caching strategy at the node.
- **Severity**: MEDIUM
- **Source**: Fly Machines docs on start time (fly.io/docs/machines/overview/#machine-start-and-stop-times)
- **Why it matters**: $300/mo projection is likely 2-3x light.

### GAP 48: LLM input token bound for queries
- **Plan section**: §9.3 "Max cost per job"
- **Missing**: Coral's query likely uses RAG with top-K wiki pages. Plan doesn't specify context window cap. A user asking "summarize the whole wiki" against a 200-page wiki = blown context window or massive cost.
- **Severity**: MEDIUM
- **Why it matters**: Per-query unbounded cost on free tier = our $500 LLM burn instead of theirs.

---

## Category H — Compliance & geographic

### GAP 49: Data residency for EU at MVP
- **Plan section**: §17.3 "Multi-region post-PMF"
- **Missing**: GDPR doesn't strictly require EU data residency, but Schrems II (CJEU 2020) significantly restricted US transfers, requiring SCCs + transfer impact assessments.
- **Severity**: MEDIUM
- **Source**: CJEU Case C-311/18 (Schrems II) + EDPB Recommendations 01/2020
- **Why it matters**: EU customers' procurement/legal will block.

### GAP 50: Log retention policy day 1
- **Plan section**: §13.3 "1 año para audit (post-PMF)"
- **Missing**: Plan says retention rules wait for post-PMF, but day-1 logging without explicit retention = data accumulates indefinitely = GDPR data minimization violation (Art. 5(1)(c) and 5(1)(e)).
- **Severity**: MEDIUM
- **Source**: GDPR Art. 5 (gdpr-info.eu/art-5-gdpr/)
- **Why it matters**: First DSAR (data subject access request) reveals years of logs you didn't know you had.

### GAP 51: Sub-processors disclosure
- **Plan section**: nowhere
- **Missing**: GDPR Art. 28 requires the controller (your customer) to know your sub-processors (Cloudflare, Anthropic, Stripe, Railway, etc.) and consent to changes. Plan doesn't include a sub-processors list in ToS/DPA.
- **Severity**: HIGH (blocks B2B)
- **Source**: GDPR Art. 28(2)
- **Why it matters**: Standard procurement checkbox.

---

## Category I — Onboarding & UX gaps

### GAP 52: Time-to-first-wiki concrete UX
- **Plan section**: §3.2 Flow A
- **Missing**: Plan implies ~5min onboarding but doesn't show: how long does bootstrap actually take? What's the loading state? Email notification when done?
- **Severity**: MEDIUM
- **Why it matters**: First impression = retention.

### GAP 53: Bootstrap failure UX
- **Plan section**: §9.3 mentions cost cap
- **Missing**: If bootstrap fails partway (hits cost cap, repo too big, parser error), what does the user see? Partial wiki? Refund of Anthropic spend? (BYOK so you can't refund what they paid Anthropic directly.) Retry button?
- **Severity**: HIGH
- **Why it matters**: First-time bootstrap failure = abandonment.

### GAP 54: Query #101 on free tier
- **Plan section**: §11.1 "100 queries/mes con nuestra key"
- **Missing**: What's the UX at query 101? Hard block? Upgrade modal? Wait until next month? Reset day-of-month edge cases?
- **Severity**: MEDIUM
- **Why it matters**: Bad upsell UX kills conversion.

### GAP 55: Org owner approval friction
- **Plan section**: §7
- **Missing**: GitHub App installation on an Org requires the org owner to approve (or owner-controlled "any member can install" setting). A team member who tries to install on the company's org will hit "request approval" friction.
- **Severity**: MEDIUM
- **Source**: GitHub Apps — managing installations
- **Why it matters**: Real-world enterprise sales funnel killer.

---

## Category J — Other findings

### GAP 56: Anthropic API output isn't valid JSON
- **Plan section**: §9.2 entrypoint reads `/tmp/output.json` via `jq`
- **Missing**: Coral's stdout is not guaranteed JSON for every subcommand. The entrypoint will silently fail on non-JSON outputs.
- **Severity**: MEDIUM
- **Why it matters**: Brittle integration with downstream worker reporting.

### GAP 57: `.bootstrap-state.json` location after S3 sync
- **Plan section**: §10.2 "Coral ya escribe `cost_usd` en `.bootstrap-state.json`"
- **Missing**: Is `.bootstrap-state.json` inside `.wiki/` (tar'd & uploaded) or in `.coral/` (separate)? If separate and not uploaded, next bootstrap loses checkpoint.
- **Severity**: MEDIUM
- **Why it matters**: Resume capability may not work cross-job.

### GAP 58: SSE behind CDN
- **Plan section**: §3.2 mentions SSE; §12.1 uses Cloudflare CDN
- **Missing**: Cloudflare's free plan has 100s response timeout; long-running SSE streams will be cut.
- **Severity**: MEDIUM
- **Source**: Cloudflare 100s rule
- **Why it matters**: Bootstrap of any repo > 100s = broken progress UI.

### GAP 59: Postgres `JSONB` for job outputs
- **Plan section**: §5.3 `jobs.output JSONB`
- **Missing**: Plan stores full LLM outputs in JSONB. No size limit. A query that returns 50KB of text per row × millions of rows = bloated table, slow VACUUM, expensive Postgres.
- **Severity**: MEDIUM
- **Source**: PostgreSQL docs on JSONB storage
- **Why it matters**: Postgres table bloat = bill shock that ruins SaaS unit economics.

### GAP 60: Cancellation flow for jobs
- **Plan section**: §9.1 lifecycle shows `cancelled` state but flow isn't described
- **Missing**: User clicks "cancel my bootstrap" → how does control plane signal Fly Machine to terminate? Mid-job termination = orphaned S3 multipart uploads, possibly orphaned LLM in-flight charges.
- **Severity**: MEDIUM
- **Why it matters**: Stuck "running" jobs are a common SaaS pain.

### GAP 61: Repo too large pre-check
- **Plan section**: §17.1 mentions size cap as risk
- **Missing**: GitHub API can give repo size via `GET /repos/{owner}/{repo}` (`size` field, in KB). Plan doesn't gate at install/repo-select step.
- **Severity**: LOW
- **Why it matters**: UX — fail fast vs fail late.

### GAP 62: GitHub Action security on Coral repo
- **Plan section**: §15 Fase 0 CI
- **Missing**: No mention of OIDC vs static tokens. Coral SaaS deploying via long-lived `RAILWAY_TOKEN` = if PR from fork runs CI and exfiltrates secret, total compromise.
- **Severity**: HIGH
- **Source**: GitHub OIDC for cloud auth (docs.github.com/en/actions/deployment/security-hardening-your-deployments/about-security-hardening-with-openid-connect)
- **Why it matters**: Classic CI/CD compromise vector.

### GAP 63: Privacy of repo metadata
- **Plan section**: §5.2 `repos.full_name TEXT`
- **Missing**: `acme-corp/secret-project` stored in plain text in DB + logs. If logs are forwarded to a third-party (Grafana Cloud), repo names leak.
- **Severity**: MEDIUM
- **Why it matters**: "We told you we use Grafana, but we didn't say it sees your repo names."

### GAP 64: Worker observability bridging
- **Plan section**: §14 "Trace IDs propagados via `traceparent`"
- **Missing**: Plan says traces propagate but worker is `bash` (per pseudo-code). No tracing library wired into the bash entrypoint. Promise vs implementation mismatch.
- **Severity**: LOW
- **Why it matters**: Observability gap claimed but not delivered.

### GAP 65: Tenant deletion / data residency in backups
- **Plan section**: nowhere
- **Missing**: When a tenant deletes, plan doesn't address: how long is their data in Postgres backups? S3 versioned objects? Some retention policies trump deletion requests (Stripe legal retention requirements override delete for invoices).
- **Severity**: MEDIUM
- **Source**: GDPR Art. 17(3)(b) — legal obligation exception
- **Why it matters**: The "right to be forgotten" has caveats that must be communicated.

### GAP 66: GitHub App marketplace listing
- **Plan section**: nowhere
- **Missing**: To get organic install traffic on GitHub Marketplace, you need a Marketplace listing — separate review, separate billing integration.
- **Severity**: LOW (growth)
- **Source**: GitHub Marketplace docs
- **Why it matters**: Distribution channel left on the table.

### GAP 67: Anthropic prompt caching
- **Plan section**: nowhere
- **Missing**: Anthropic offers prompt caching that reduces cost by ~90% for repeated context. Coral's wiki context is the perfect cache candidate. Plan doesn't mention.
- **Severity**: LOW (cost optimization)
- **Source**: Anthropic prompt caching docs (docs.anthropic.com/en/docs/build-with-claude/prompt-caching)
- **Why it matters**: Free tier banking could be 5-10x cheaper.

### GAP 68: Secret leakage in repo bootstrap
- **Plan section**: nowhere
- **Missing**: User's repo may contain hardcoded secrets (.env files, API keys in test fixtures). Coral reads them, includes excerpts in wiki, wiki is stored in S3 with potentially weaker controls than the source repo had. Trufflehog-style pre-scan not mentioned.
- **Severity**: HIGH
- **Source**: OWASP Top 10 2025 A02 (Cryptographic Failures)
- **Why it matters**: One leaked wiki = real customer data + their secrets exposed.

### GAP 69: Anthropic model availability per region
- **Plan section**: nowhere
- **Missing**: Anthropic API has regional gating and quota differences. Plan doesn't address what happens if user's region has different model availability.
- **Severity**: LOW
- **Why it matters**: User selects model, doesn't work in their geography = mysterious failure.

### GAP 70: Internal admin access controls
- **Plan section**: §13.1 covers user-vs-user but not admin
- **Missing**: As the operator, you (and future employees) can access any tenant's wiki via DB query / S3 console. No mention of just-in-time access, break-glass procedure, customer notification of admin access.
- **Severity**: MEDIUM
- **Source**: SOC 2 CC6.1, CC6.3
- **Why it matters**: First enterprise prospect's question: "who at your company can see my data?"

---

## TOP 10 RISKS (Synthesis)

Ranked by combined likelihood × impact for a real production launch:

1. **GAP 19 — RLS + pgbouncer pool mode**: Silent cross-tenant leakage is the #1 SaaS disaster. The plan's claim of RLS-as-defense-in-depth is undermined if `SET LOCAL` is used incorrectly with transaction-pool mode.

2. **GAP 9 — Stripe webhook handling absent**: Without subscription lifecycle reconciliation, the plan's revenue model leaks money from day 1 of paid tier launch.

3. **GAP 36 — Worker container hardening**: Runs as root, no seccomp, no read-only FS, on untrusted user code. The implement loop is explicitly designed to run untrusted generated code in this container.

4. **GAP 30 — CSRF on state-changing endpoints**: Cookie-based auth without CSRF defense = trivial attack draining user LLM budgets, deleting wikis, or worse.

5. **GAP 16 — GDPR right-to-delete cascade**: Plan promises a "delete-my-data endpoint" but doesn't address backups, S3 versions, Stripe records, or observability traces.

6. **GAP 35 — No audit log at MVP**: First security incident with no audit trail = inability to scope blast radius. First enterprise prospect dies on this.

7. **GAP 32 — Long-lived worker bearer token**: A single static token gating access to internal control-plane APIs is a single point of compromise.

8. **GAP 68 — Secret leakage from source into wiki**: Coral indexes everything in the repo, including `.env.example`, hardcoded keys in test fixtures, and ends up persisted in S3 in plaintext.

9. **GAP 62 — GitHub Action deploy secrets**: Long-lived `RAILWAY_TOKEN`/`FLY_API_TOKEN` in CI without OIDC = full prod compromise from any PR that runs the right workflow.

10. **GAP 3 — `installation.deleted` data deletion policy**: Uninstall is the user's de facto deletion signal. Silently retaining their wiki = ToS/GDPR violation.
