-- Coral-SAAS — initial schema (Fase 1 of SAAS-PLAN.md §5).
-- Apply with:  sqlx migrate run

CREATE EXTENSION IF NOT EXISTS "pgcrypto";  -- for gen_random_uuid()

-- ------------------------------------------------------------------
-- Identity
-- ------------------------------------------------------------------

CREATE TABLE tenants (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    slug                TEXT UNIQUE NOT NULL,
    name                TEXT NOT NULL,
    plan                TEXT NOT NULL DEFAULT 'free'
        CHECK (plan IN ('free', 'pro', 'team', 'enterprise')),
    stripe_customer_id  TEXT,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at          TIMESTAMPTZ
);

CREATE TABLE users (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    github_id     BIGINT UNIQUE NOT NULL,
    github_login  TEXT NOT NULL,
    email         TEXT,
    avatar_url    TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE tenant_members (
    tenant_id  UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role       TEXT NOT NULL
        CHECK (role IN ('owner', 'admin', 'member')),
    joined_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, user_id)
);

CREATE TABLE sessions (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX sessions_expires_idx ON sessions(expires_at) WHERE expires_at > now();

-- ------------------------------------------------------------------
-- GitHub integration (§5.2)
-- ------------------------------------------------------------------

CREATE TABLE github_installations (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id        UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    installation_id  BIGINT UNIQUE NOT NULL,
    account_login    TEXT NOT NULL,
    account_type     TEXT NOT NULL CHECK (account_type IN ('User', 'Organization')),
    installed_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    suspended_at     TIMESTAMPTZ,
    disconnected_at  TIMESTAMPTZ
);

CREATE TABLE repos (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id           UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    installation_id     UUID NOT NULL REFERENCES github_installations(id) ON DELETE CASCADE,
    github_repo_id      BIGINT UNIQUE NOT NULL,
    full_name           TEXT NOT NULL,
    default_branch      TEXT NOT NULL,
    last_indexed_sha    TEXT,
    wiki_s3_key         TEXT,
    embeddings_s3_key   TEXT,
    bootstrap_status    TEXT NOT NULL DEFAULT 'pending'
        CHECK (bootstrap_status IN ('pending', 'running', 'ready', 'failed')),
    bootstrap_cost_usd  NUMERIC(10, 4) DEFAULT 0,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    disconnected_at     TIMESTAMPTZ
);

-- ------------------------------------------------------------------
-- Jobs & usage (§5.3)
-- ------------------------------------------------------------------

CREATE TABLE jobs (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id       UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    repo_id         UUID REFERENCES repos(id) ON DELETE CASCADE,
    user_id         UUID REFERENCES users(id),
    kind            TEXT NOT NULL
        CHECK (kind IN ('bootstrap', 'ingest', 'query', 'lint', 'implement')),
    status          TEXT NOT NULL DEFAULT 'queued'
        CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'cancelled')),
    input           JSONB NOT NULL,
    output          JSONB,
    error           TEXT,
    failure_reason  TEXT,   -- enumerated reason for UX (see SAAS-PLAN §15 Fase 5)
    cost_usd        NUMERIC(10, 4) DEFAULT 0,
    input_tokens    INT,
    output_tokens   INT,
    duration_ms     INT,
    queued_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at      TIMESTAMPTZ,
    finished_at     TIMESTAMPTZ
);

CREATE INDEX jobs_tenant_kind_status_idx ON jobs(tenant_id, kind, status);
CREATE INDEX jobs_queued_idx ON jobs(status, queued_at) WHERE status = 'queued';
CREATE INDEX jobs_repo_finished_idx ON jobs(repo_id, finished_at DESC);

CREATE TABLE usage_ledger (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id   UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    job_id      UUID REFERENCES jobs(id),
    period      TEXT NOT NULL,
    cost_usd    NUMERIC(10, 4) NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX usage_ledger_tenant_period_idx ON usage_ledger(tenant_id, period);

-- ------------------------------------------------------------------
-- Secrets (BYOK) (§5.4)
-- ------------------------------------------------------------------

CREATE TABLE tenant_secrets (
    tenant_id                       UUID PRIMARY KEY REFERENCES tenants(id) ON DELETE CASCADE,
    anthropic_api_key_ciphertext    BYTEA,
    voyage_api_key_ciphertext       BYTEA,
    key_version                     INT NOT NULL DEFAULT 1,
    updated_at                      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ------------------------------------------------------------------
-- Audit log (§13.4) — at MVP, not post-PMF
-- ------------------------------------------------------------------

CREATE TABLE audit_events (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id         UUID REFERENCES tenants(id),
    actor_user_id     UUID REFERENCES users(id),
    actor_type        TEXT NOT NULL
        CHECK (actor_type IN ('user', 'system', 'operator', 'webhook_github', 'webhook_stripe')),
    action            TEXT NOT NULL,
    resource_kind     TEXT,
    resource_id       TEXT,
    metadata          JSONB,
    legal_retention   BOOLEAN NOT NULL DEFAULT FALSE,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX audit_events_tenant_idx ON audit_events(tenant_id, created_at DESC);
CREATE INDEX audit_events_actor_idx  ON audit_events(actor_user_id, created_at DESC);

-- ------------------------------------------------------------------
-- Stripe webhook dedup (§11.3)
-- ------------------------------------------------------------------

CREATE TABLE stripe_events (
    id            TEXT PRIMARY KEY,      -- evt_XXX from Stripe
    processed_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ------------------------------------------------------------------
-- RLS — defence in depth (§5.5)
-- See CLAUDE.md "Critical invariants" about pgbouncer.
-- ------------------------------------------------------------------

ALTER TABLE repos             ENABLE ROW LEVEL SECURITY;
ALTER TABLE jobs              ENABLE ROW LEVEL SECURITY;
ALTER TABLE usage_ledger      ENABLE ROW LEVEL SECURITY;
ALTER TABLE github_installations ENABLE ROW LEVEL SECURITY;
ALTER TABLE tenant_secrets    ENABLE ROW LEVEL SECURITY;
ALTER TABLE audit_events      ENABLE ROW LEVEL SECURITY;

CREATE POLICY tenant_isolation_repos ON repos
    USING (tenant_id = current_setting('app.tenant_id', true)::uuid);

CREATE POLICY tenant_isolation_jobs ON jobs
    USING (tenant_id = current_setting('app.tenant_id', true)::uuid);

CREATE POLICY tenant_isolation_usage ON usage_ledger
    USING (tenant_id = current_setting('app.tenant_id', true)::uuid);

CREATE POLICY tenant_isolation_installations ON github_installations
    USING (tenant_id = current_setting('app.tenant_id', true)::uuid);

CREATE POLICY tenant_isolation_secrets ON tenant_secrets
    USING (tenant_id = current_setting('app.tenant_id', true)::uuid);

-- audit_events has both tenant-scoped rows AND system-wide rows (operator actions).
-- Tenant users see only their own; operator role bypasses RLS.
CREATE POLICY tenant_isolation_audit ON audit_events
    USING (tenant_id IS NULL OR tenant_id = current_setting('app.tenant_id', true)::uuid);
