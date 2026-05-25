# Coral SaaS — Plan de Arquitectura

**Status**: DRAFT v0.3 — decisiones de stack confirmadas (Next.js + Railway-everywhere)
**Destino**: https://github.com/agustincbajo/Coral-SAAS (repo nuevo)
**Fecha**: 2026-05-24
**Autor**: Agustín Bajo (con asistencia de Claude)
**Reviewers**: ver `docs/SAAS-PLAN-GAPS.md` para los 70 gaps identificados.

---

## 0. Resumen ejecutivo

Convertir Coral (CLI Rust single-tenant filesystem-native) en un SaaS multi-tenant donde cualquier dev/equipo conecta su repo de GitHub vía GitHub App, Coral genera y mantiene un wiki AI-readable del codebase, y lo expone vía web UI con queries por LLM, lint, e implement loop.

**Estrategia arquitectónica recomendada**: **Wrapper SaaS + Coral as worker subprocess**. NO forkar Coral. El servicio web (control plane) maneja auth/billing/GitHub/orquestación; el binario `coral` corre en containers efímeros como worker, con `.wiki/` y código fuente montados desde storage per-tenant. Esto respeta la arquitectura actual de Coral (atomic_write + flock, single wiki root, filesystem nativo) y minimiza divergencia upstream.

**MVP target**: 6-8 semanas para usuario único + GitHub App + bootstrap + read-only wiki + queries básicas. Pre-revenue.

---

## 0.5 Cambios desde v0.1 (post-review)

### Verificado por agente técnico (19/19 claims correctas)
Todo lo afirmado sobre el código actual de Coral en v0.1 fue verificado contra el repo. Cero correcciones técnicas necesarias.

### Cambios estructurales aplicados desde v0.1
Los gaps con **severidad HIGH** del review se incorporaron al plan:

| Gap (ver `SAAS-PLAN-GAPS.md`) | Sección modificada | Cambio |
|------|--------------------|--------|
| #19 RLS + pgbouncer | §5.5 | Pool mode obligatorio session (no transaction), patrón request → middleware tx |
| #20 S3 IAM scoping | §6.1 | Decidido: pre-signed URLs minted por control plane (R2-compatible) |
| #21 Cross-tenant cache | §3.x | Cache keys SIEMPRE prefijadas con tenant_id |
| #30 CSRF | §8.4 nuevo | SameSite=Strict + double-submit cookie + Origin check |
| #32 WORKER_TOKEN rotation | §9.2 | Token corto firmado per-job (no estático) |
| #36 Container hardening | §9.2 | USER non-root + read-only FS + cap-drop=all + pinned digests |
| #35 Audit log | §13.4 nuevo | `audit_events` table al MVP, no post-PMF |
| #16 GDPR cascade | §13.2 | Procedure documentada: DB + R2 + logs + Stripe + observability |
| #9 Stripe webhooks | §11.3 nuevo | Lifecycle handler con dedup en `stripe_events` |
| #12 VAT/tax | §11.4 nuevo | Stripe Tax desde día 1 si vendemos en EU |
| #46 Free tier abuse | §11.2 | Card-on-file required + fingerprinting + cap absoluto |
| #51 Sub-processors | §13.5 nuevo | Lista pública en `/legal/sub-processors` |
| #53 Bootstrap failure UX | §15 Fase 5 | Retry + parcial-state visible + email notify |
| #62 GitHub Action OIDC | §15 Fase 0 | OIDC en CI, no PATs estáticos |
| #68 Secret leakage | §10.4 nuevo | Trufflehog pre-bootstrap, abort si encuentra secret de alta confianza |
| #3 installation.deleted | §7.5 nuevo | Grace period 30 días + email + opcional purge inmediato |
| #5 Permission re-consent | §15 Fase v2 | Re-consent flow planificado antes del feature flip |
| #23 Postgres backups | §13.6 nuevo | Neon PITR 7d + monthly restore test |
| #15 ToS/Privacy detail | §13.5 | Templates referenciados, no checkbox vacío |

### Gaps MEDIUM/LOW aún pendientes
51 gaps de severidad media o baja documentados en `SAAS-PLAN-GAPS.md` — se priorizan post-MVP o al toparse en una fase. No bloquean inicio de desarrollo.

### Cambios v0.2 → v0.3 (decisiones del usuario)

- **Repo**: confirmado separado en https://github.com/agustincbajo/Coral-SAAS
- **Free tier**: confirmado con nuestra API key (no BYOK obligatorio al MVP). Ver §11.2.
- **Frontend stack**: Next.js 15 + React 18 + TanStack Query + Zustand + Tailwind 4 + `@playsistemico/modo-bo-ui-lib` (UI lib privada). Tomado de `em-dashboard` + `modo-bo-ui-lib` como referencia (§4 actualizado).
- **Postgres**: Railway Postgres add-on (no Neon). Trade-off: pierde PITR, pero gana simplicidad operativa (un solo proveedor). Plan B: WAL archive a R2 si necesitamos PITR en v1. Ver §12 y §13.6.
- **Worker hosting**: Railway service long-running con queue polling (no Fly Machines). Trade-off: pierde scale-to-zero, pero gana monoproveedor + DATABASE_URL injection nativa. Ver §9.4 reescrita.
- **Estructura repo**: monorepo estilo `em-dashboard` con `api/` (Rust Axum) + `web/` (Next.js) + `worker/` (Rust binary) + `shared/` (crate común), un Dockerfile por servicio, `railway.toml` con tres `[[services]]`. Ver §4.1.

---

## 1. Visión y scope

### 1.1 ¿Qué problema resuelve?

Hoy, para que un equipo use Coral:
1. Cada dev instala el binario localmente
2. Cada dev corre `coral setup` (cuesta ~$0.10–$1 en LLM por bootstrap)
3. El `.wiki/` queda en su filesystem local, desfasado de los compañeros
4. Cada uno paga su propia API key

Para un equipo de 10 devs es ridículo. El wiki debería ser un **artefacto compartido** que se mantiene server-side y todos consultan.

### 1.2 No-goals (para el MVP)

- **No-goal**: Reemplazar a Coral CLI local. El SaaS y el CLI conviven.
- **No-goal**: Soportar repos privados de >100k LOC en el MVP (ingest demasiado caro).
- **No-goal**: Editor colaborativo tipo Notion. El wiki se edita vía PR a Git o desde la UI (single-writer).
- **No-goal**: Bajar latencia de query a <500ms. LLM tarda lo que tarda.
- **No-goal**: On-premise / self-hosted. Eso queda para Enterprise tier post-PMF.

### 1.3 Tiers de scope

| Tier | Capacidades | Timeline |
|------|-------------|----------|
| **MVP** | 1 user, 1 repo, bootstrap + read-only wiki + queries vía UI | Semanas 1–8 |
| **v1** | Multi-user (org), múltiples repos, lint con auto-fix, GitHub PR comments | Semanas 9–16 |
| **v2** | Implement loop completo (UNDERSTAND→APPLY vía PR), billing Stripe, free/pro tiers | Semanas 17–28 |
| **v3** | Webhook ingest incremental, Slack notifications, embeddings con Voyage AI | Mes 7+ |
| **Enterprise** | SSO (SAML), SOC2, audit log, self-hosted option, BYOK obligatorio | Mes 12+ |

---

## 2. Realidad técnica de Coral (verificada, no inventada)

Antes de planificar, anclamos a hechos del código actual (basado en mapeo del Explore agent contra `/Users/agustinbajo/Documents/GitHub/Coral`).

### 2.1 Lo que Coral YA TIENE — útil para el SaaS

| Capacidad | Ubicación | Implicación para SaaS |
|-----------|-----------|------------------------|
| Single binary con SPA embebida | `crates/coral-ui` | Empacable en Docker en una sola imagen pequeña |
| Auth bearer token off-loopback | `crates/coral-ui/src/auth.rs:44-49` | Podemos usarlo como auth interno entre control-plane y worker |
| Provider chain (`--provider` → `CORAL_PROVIDER` → `CLAUDECODE` → `Claude`) | `crates/coral-cli/src/commands/runner_helper.rs:353-372` | Pasar `CORAL_PROVIDER=anthropic_api` + `ANTHROPIC_API_KEY` por tenant |
| `AnthropicApiRunner` que habla directo a `api.anthropic.com` | `crates/coral-runner/src/anthropic_api.rs` | **NO hace falta instalar `claude` CLI en la imagen Docker** |
| Bootstrap con checkpoint + resume + `max_cost_usd` | `crates/coral-cli/src/commands/bootstrap/state.rs` | Podemos enforcear quota por tenant pasando `--max-cost` |
| Atomic writes con flock(2) | `crates/coral-core/src/atomic.rs:164-199` | Funciona dentro de un container pero **no cross-host** |
| Embeddings en SQLite `.coral-embeddings.db` | `crates/coral-core/src/embeddings_sqlite.rs:29-32` | Portable, se puede copiar/respaldar trivialmente |
| MCP server stdio-only | `crates/coral-mcp/src/server.rs:13` | Útil si exponemos Coral como MCP server al cliente |

### 2.2 Lo que Coral NO TIENE — gaps que el SaaS DEBE cubrir

| Gap | Citas | Plan |
|-----|-------|------|
| Concepto de `tenant_id` / `user_id` | No existe en ningún módulo (`coral-cli`, `coral-core`, `coral-ui`) | Tenant vive solo en el control plane; worker corre en sandbox físico per-tenant |
| Distributed locking | flock(2) es per-host (`atomic.rs:186-195`) | **Una operación = un container = un host**. Serializamos via job queue, no via lock distribuido |
| Multi-provider per-tenant | Provider es global per-process (`runner_helper.rs:353-372`) | Cada worker arranca con env vars del tenant que lo invocó |
| Webhook receiver | Inexistente | Control plane (Node/Rust) recibe webhooks de GitHub |
| Persistent sessions / OAuth | Solo bearer tokens estáticos (`auth.rs:51`) | Control plane usa GitHub OAuth + sesiones (cookies firmadas) |
| Cost tracking per-tenant | Solo `max_cost_usd` por bootstrap (`bootstrap/mod.rs:38`) | Control plane lee `cost_spent_usd` del `.bootstrap-state.json` al final de cada job |
| Rate limiting | Solo `BOOTSTRAP_MAX_PARALLEL=4` (`bootstrap/mod.rs:25`) | Control plane + token bucket per tenant_id |

### 2.3 La pregunta crítica: ¿se aísla por proceso o por container?

**Recomendación: container per job**. Razones:

1. **Filesystem assumption**: Coral asume root único (`.wiki/`, `.coral/`, `.coral-embeddings.db`). Cambiar `--wiki-root` por job es trivial; el resto se queda igual.
2. **flock(2)**: dentro de un container, el lock funciona. Si dos jobs del mismo tenant compiten, los serializamos en la cola (no en el filesystem).
3. **Sandbox**: el código fuente del tenant se monta read-only. Si el implement loop genera algo malicioso, el blast radius está limitado al container efímero.
4. **Sin fork de Coral**: usamos el binario tal cual, con env vars y flags. Cualquier upstream merge es directo.

---

## 3. Arquitectura general

```
┌──────────────────────────────────────────────────────────────────────────┐
│                            USUARIO (browser)                              │
└────────────────────────────────┬─────────────────────────────────────────┘
                                 │ HTTPS
                                 ▼
┌──────────────────────────────────────────────────────────────────────────┐
│                    CONTROL PLANE (Rust + Axum)                            │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌────────────┐ │
│  │  Web UI  │  │  REST    │  │  GitHub  │  │  Auth    │  │  Billing   │ │
│  │  (SPA)   │  │  API     │  │  Webhook │  │  (OAuth) │  │  (Stripe)  │ │
│  └──────────┘  └──────────┘  └──────────┘  └──────────┘  └────────────┘ │
└────────┬───────────────────┬─────────────────┬─────────────────┬─────────┘
         │                   │                 │                 │
         ▼                   ▼                 ▼                 ▼
   ┌──────────┐       ┌────────────┐    ┌──────────┐      ┌──────────┐
   │ Postgres │       │  S3-compat │    │ Job queue│      │  Stripe  │
   │ (tenant, │       │ (wiki.tar, │    │  (Redis/ │      │   API    │
   │  user,   │       │  embeddings│    │  SQS)    │      │          │
   │  usage)  │       │  per-tenant│    │          │      │          │
   └──────────┘       └────────────┘    └────┬─────┘      └──────────┘
                                              │
                                              │ pull job
                                              ▼
                  ┌────────────────────────────────────────┐
                  │       WORKER POOL (Kubernetes Job)      │
                  │  ┌──────────────────────────────────┐   │
                  │  │  Per-job ephemeral container:    │   │
                  │  │  - coral binary                  │   │
                  │  │  - git clone <tenant repo>       │   │
                  │  │  - fetch .wiki/ from S3          │   │
                  │  │  - run command (bootstrap/ingest/│   │
                  │  │    query/lint/implement)         │   │
                  │  │  - push .wiki/ back to S3        │   │
                  │  │  - report cost + status to ctrl  │   │
                  │  └──────────────────────────────────┘   │
                  └────────────────────────────────────────┘
```

### 3.1 Componentes por responsabilidad

**Control Plane**:
- Recibe requests del usuario y de GitHub
- Autentica, autoriza, contabiliza
- Encola jobs y lee resultados
- Sirve el wiki (read-only) directamente desde S3+cache
- NO ejecuta `coral` en sí mismo

**Worker pool**:
- Procesos efímeros (un container = un job)
- Estado 100% en S3 + Postgres; el container muere al terminar
- Único componente que ejecuta el binario `coral`
- Usa la API key de Anthropic del tenant (BYOK)

**Persistence**:
- Postgres: metadata, usage, sesiones, billing
- S3: artefactos pesados (wiki, embeddings, source snapshots opcionales)
- Redis/SQS: cola de jobs

### 3.2 Flujos críticos

**Flujo A — Onboarding**:
1. User va a `coral-saas.com`, click "Sign in with GitHub" → OAuth
2. Control plane crea `tenant` y `user`
3. Modal: "Install Coral GitHub App" → redirect a GitHub
4. User selecciona repos → callback a `/api/github/install`
5. Control plane guarda `installation_id`, lista los repos
6. User elige uno → "Run bootstrap" → encola job `bootstrap(tenant_id, repo_id)`
7. Worker clona repo, corre `coral setup --max-cost 2.00`, sube `.wiki/` a S3
8. UI muestra progreso en vivo (SSE desde control plane leyendo Postgres)
9. Al terminar, redirige a `/w/<tenant>/<repo>` con el wiki renderizado

**Flujo B — Query**:
1. User abre wiki, click "Ask question", escribe "how does auth work?"
2. POST `/api/query` con `{repo_id, question}`
3. Control plane encola job `query(tenant_id, repo_id, question)`
4. Worker arranca container, descarga `.wiki/` desde S3 (cacheable), corre `coral query "how does auth work?"`, devuelve respuesta
5. Control plane stream-respondea (SSE) al usuario
6. Cost se debita del balance del tenant

**Flujo C — Sync incremental (post-MVP)**:
1. GitHub manda webhook `push` al control plane
2. Control plane verifica firma HMAC del webhook
3. Si el repo está conectado, encola job `ingest(tenant_id, repo_id, commit_sha)`
4. Worker pulla cambios, corre `coral ingest --since <prev_sha>`, sube delta a S3
5. Control plane invalida cache del wiki en CDN

---

## 4. Tech stack (v0.3 — confirmado)

| Capa | Tech | Justificación |
|------|------|---------------|
| **Control plane (API)** | Rust + Axum + Tokio | Mismo lenguaje que Coral, type safety, perf |
| **DB** | PostgreSQL 16 vía Railway Postgres add-on | DATABASE_URL inyectada automáticamente, simple |
| **ORM** | sqlx | Compile-time checked queries, async nativo |
| **Job queue** | Redis (Railway add-on) + `apalis` (Rust) | Pub/sub + persistent queue en un solo backend |
| **Object storage** | Cloudflare R2 (S3-compat) | No cobra egress → wiki reads cuestan ~$0 |
| **Worker runtime** | Railway service long-running, poll-based (1+ replicas) | Mismo proveedor que api/web, ver §9.4 |
| **Frontend** | **Next.js 15 + React 18 + TanStack Query + Zustand + Tailwind 4** | Stack idéntico al de `em-dashboard`, conoce el patrón |
| **UI components** | **`@playsistemico/modo-bo-ui-lib`** (instalada via GitHub Packages) | Reuso del design system que ya mantenés |
| **Auth** | GitHub OAuth + sesiones cookie-firmadas | Mismo patrón que em-dashboard |
| **Billing** | Stripe (Checkout + Customer Portal + webhooks) | Standard para SaaS |
| **CDN/edge** | Cloudflare (DNS + CDN + WAF) | DDoS gratis + SSE bypass en subdomain dedicado |
| **Email transac.** | Resend | Free tier 3k/mes |
| **Observability** | OpenTelemetry + Grafana Cloud (free tier) | Trace jobs end-to-end + structured logs |
| **CI/CD** | GitHub Actions con OIDC → Railway deploy | Mismo patrón que em-dashboard |

### 4.1 Estructura del repo Coral-SAAS

Idéntica al patrón de `em-dashboard` (monorepo con services separados, cada uno con su Dockerfile, deploy unificado via `railway.toml`):

```
Coral-SAAS/
├── README.md
├── CLAUDE.md                    # instrucciones para Claude Code
├── railway.toml                 # define 3 services: api, web, worker
├── docker-compose.yml           # dev local: postgres + redis + api + web + worker
├── .github/
│   └── workflows/
│       ├── ci.yml               # cargo test, cargo clippy, trivy, web typecheck
│       └── deploy.yml           # OIDC → Railway (no static tokens)
├── docs/
│   ├── SAAS-PLAN.md             # este doc
│   ├── SAAS-PLAN-GAPS.md
│   └── ARCHITECTURE.md
├── api/                         # Rust control plane (Axum)
│   ├── Cargo.toml
│   ├── Dockerfile
│   ├── src/
│   │   ├── main.rs
│   │   ├── routes/              # /auth, /api/repos, /api/jobs, /api/stripe, /api/github
│   │   ├── auth/                # sessions, GitHub OAuth
│   │   ├── github_app/          # JWT signing, installation tokens, webhook verifier
│   │   ├── stripe/              # webhook handler, subscription sync
│   │   ├── jobs/                # enqueue + status (worker consumes from Redis)
│   │   ├── db/                  # sqlx queries
│   │   └── middleware/          # tenant_id RLS injection, CSRF, rate limit
│   ├── migrations/              # sqlx migrations (numbered .sql files)
│   └── tests/                   # integration tests with testcontainers
├── worker/                      # Rust queue consumer + coral subprocess runner
│   ├── Cargo.toml
│   ├── Dockerfile               # baked coral binary + trufflehog + git
│   └── src/
│       ├── main.rs              # poll Redis, spawn coral child, report back
│       ├── coral_runner.rs      # subprocess wrapper
│       ├── s3.rs                # pre-signed URL fetch + upload
│       └── secret_scan.rs       # trufflehog wrapper
├── web/                         # Next.js 15 frontend
│   ├── package.json             # next, react, tanstack/react-query, zustand, tailwind, @playsistemico/modo-bo-ui-lib
│   ├── Dockerfile
│   ├── next.config.mjs
│   ├── tailwind.config.ts
│   ├── tsconfig.json
│   ├── .npmrc                   # GitHub Packages registry config
│   └── src/
│       ├── app/                 # App router
│       │   ├── (auth)/login/
│       │   ├── (dashboard)/
│       │   │   ├── repos/
│       │   │   ├── repos/[id]/
│       │   │   ├── repos/[id]/wiki/[...slug]/
│       │   │   └── settings/
│       │   └── api/             # Next.js API routes (proxy to /api en api service o auth callbacks)
│       ├── components/          # composiciones específicas de Coral
│       ├── lib/                 # api client, session helpers
│       └── styles/
└── shared/                      # Rust crate común a api + worker
    ├── Cargo.toml
    └── src/
        └── lib.rs               # tipos compartidos: JobSpec, JobResult, errors
```

### 4.2 `railway.toml` propuesto

Tres `[[services]]` apuntando al mismo repo + Railway Postgres + Railway Redis add-ons:

```toml
[[services]]
name = "api"
root = "api"

[services.build]
builder = "DOCKERFILE"
dockerfilePath = "Dockerfile"

[services.deploy]
healthcheckPath = "/healthz"
restartPolicyType = "ON_FAILURE"
restartPolicyMaxRetries = 5

[[services]]
name = "web"
root = "web"

[services.build]
builder = "DOCKERFILE"
dockerfilePath = "Dockerfile"

[services.deploy]
healthcheckPath = "/api/health"
restartPolicyType = "ON_FAILURE"
restartPolicyMaxRetries = 5

[[services]]
name = "worker"
root = "worker"

[services.build]
builder = "DOCKERFILE"
dockerfilePath = "Dockerfile"

[services.deploy]
restartPolicyType = "ALWAYS"
restartPolicyMaxRetries = 0
# No healthcheck — worker tiene su propio liveness via stdout heartbeat
```

Variables compartidas (configuradas en Railway dashboard):
- `DATABASE_URL` (inyectada por Postgres add-on, shared a api + worker)
- `REDIS_URL` (inyectada por Redis add-on)
- `R2_ACCESS_KEY_ID`, `R2_SECRET_ACCESS_KEY`, `R2_BUCKET`, `R2_ENDPOINT`
- `ANTHROPIC_API_KEY` (nuestra key para free tier — solo en worker)
- `GITHUB_APP_ID`, `GITHUB_APP_PRIVATE_KEY`, `GITHUB_WEBHOOK_SECRET`
- `GITHUB_OAUTH_CLIENT_ID`, `GITHUB_OAUTH_CLIENT_SECRET` (separados del App si decidimos dual)
- `STRIPE_SECRET_KEY`, `STRIPE_WEBHOOK_SECRET`
- `SESSION_SECRET` (firma de cookies)
- `WORKER_JWT_SECRET` (para JWTs cortos api → worker, §9.2)
- `RESEND_API_KEY`
- `OTEL_EXPORTER_OTLP_ENDPOINT`, `OTEL_EXPORTER_OTLP_HEADERS`

### 4.3 Frontend UI library reuse

El `@playsistemico/modo-bo-ui-lib` ya existe y tiene componentes para back-office (tablas, forms, modales, layouts dashboard). Para Coral-SAAS:

1. `.npmrc` apunta a `@playsistemico:registry=https://npm.pkg.github.com/` con `_authToken` (PAT con `read:packages` scope, en Railway env)
2. `import { Button, Modal, Sidebar, ... } from '@playsistemico/modo-bo-ui-lib'`
3. Si falta algún componente específico de Coral (ej. wiki page renderer, query chat), lo creamos en `web/src/components/` siguiendo el mismo design system

Esto acelera onboarding: layouts, navegación, formularios = sin reinventar.

---

## 5. Data model (PostgreSQL)

Schema inicial. Todas las tablas tienen `id UUID DEFAULT gen_random_uuid()`, `created_at`, `updated_at`.

### 5.1 Identity

```sql
CREATE TABLE tenants (
  id UUID PRIMARY KEY,
  slug TEXT UNIQUE NOT NULL,                 -- 'acme-corp' (URL-safe)
  name TEXT NOT NULL,
  plan TEXT NOT NULL DEFAULT 'free',         -- 'free' | 'pro' | 'team' | 'enterprise'
  stripe_customer_id TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE users (
  id UUID PRIMARY KEY,
  github_id BIGINT UNIQUE NOT NULL,
  github_login TEXT NOT NULL,
  email TEXT,
  avatar_url TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE tenant_members (
  tenant_id UUID REFERENCES tenants(id) ON DELETE CASCADE,
  user_id UUID REFERENCES users(id) ON DELETE CASCADE,
  role TEXT NOT NULL,                        -- 'owner' | 'admin' | 'member'
  PRIMARY KEY (tenant_id, user_id)
);
```

### 5.2 GitHub integration

```sql
CREATE TABLE github_installations (
  id UUID PRIMARY KEY,
  tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  installation_id BIGINT UNIQUE NOT NULL,    -- de GitHub
  account_login TEXT NOT NULL,               -- 'agustincbajo' o 'acme-corp'
  account_type TEXT NOT NULL,                -- 'User' | 'Organization'
  installed_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE repos (
  id UUID PRIMARY KEY,
  tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  installation_id UUID NOT NULL REFERENCES github_installations(id) ON DELETE CASCADE,
  github_repo_id BIGINT UNIQUE NOT NULL,
  full_name TEXT NOT NULL,                   -- 'agustincbajo/Coral'
  default_branch TEXT NOT NULL,
  last_indexed_sha TEXT,
  wiki_s3_key TEXT,                          -- 's3://bucket/tenants/<id>/repos/<id>/wiki.tar.zst'
  embeddings_s3_key TEXT,
  bootstrap_status TEXT NOT NULL DEFAULT 'pending',  -- 'pending' | 'running' | 'ready' | 'failed'
  bootstrap_cost_usd NUMERIC(10,4) DEFAULT 0,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

### 5.3 Jobs & usage

```sql
CREATE TABLE jobs (
  id UUID PRIMARY KEY,
  tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  repo_id UUID REFERENCES repos(id) ON DELETE CASCADE,
  user_id UUID REFERENCES users(id),         -- quien lo disparó (null si es webhook)
  kind TEXT NOT NULL,                        -- 'bootstrap' | 'ingest' | 'query' | 'lint' | 'implement'
  status TEXT NOT NULL DEFAULT 'queued',     -- 'queued' | 'running' | 'succeeded' | 'failed' | 'cancelled'
  input JSONB NOT NULL,                      -- {question: '...', max_cost: 2.00, ...}
  output JSONB,                              -- {answer: '...', sources: [...], ...}
  error TEXT,
  cost_usd NUMERIC(10,4) DEFAULT 0,
  input_tokens INT,
  output_tokens INT,
  duration_ms INT,
  queued_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  started_at TIMESTAMPTZ,
  finished_at TIMESTAMPTZ
);

CREATE INDEX jobs_tenant_kind_status_idx ON jobs(tenant_id, kind, status);
CREATE INDEX jobs_queued_idx ON jobs(status, queued_at) WHERE status = 'queued';

CREATE TABLE usage_ledger (
  id UUID PRIMARY KEY,
  tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  job_id UUID REFERENCES jobs(id),
  period TEXT NOT NULL,                      -- '2026-05' (year-month for billing rollup)
  cost_usd NUMERIC(10,4) NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX usage_ledger_tenant_period_idx ON usage_ledger(tenant_id, period);
```

### 5.4 Secrets (BYOK)

```sql
CREATE TABLE tenant_secrets (
  tenant_id UUID PRIMARY KEY REFERENCES tenants(id) ON DELETE CASCADE,
  anthropic_api_key_ciphertext BYTEA,        -- AES-256-GCM, KMS-derived key
  voyage_api_key_ciphertext BYTEA,
  key_version INT NOT NULL DEFAULT 1,
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

API keys se encriptan en reposo con AWS KMS (o equivalente). Nunca se loguean; se desencriptan justo antes de inyectarlas como env var al container del worker.

### 5.5 RLS (Row-Level Security)

Postgres RLS para que un bug accidental no permita query cross-tenant:

```sql
ALTER TABLE repos ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON repos USING (tenant_id = current_setting('app.tenant_id')::uuid);
```

**Implementación crítica (corregido por GAP #19)**:

- **NO usar pgbouncer en modo `transaction`**. `SET LOCAL` solo vive dentro de una tx; en pool transaction-mode las conexiones se reciclan y cualquier `SELECT` fuera de una tx explícita pierde el `tenant_id` → RLS queda inactiva silenciosamente.
- **Opciones aceptables**:
  - (a) Pool en modo `session` (cada user-request reusa misma conn, no comparte). Más caro en pool size.
  - (b) Modo `transaction` PERO middleware obliga a abrir una tx para todo handler, y `SET LOCAL app.tenant_id` es lo primero dentro de la tx. Test obligatorio: `SHOW app.tenant_id` al inicio de cada query debe matchear el del request.
- **Recomendación**: opción (b) + test de integración que falle el build si algún handler hace queries fuera de tx.

Defense in depth: la lógica de aplicación también filtra por `tenant_id` explícitamente, pero RLS es el último guardrail.

---

## 6. Multi-tenancy & isolation

### 6.1 Capas de aislamiento

| Capa | Mecanismo | Protege contra |
|------|-----------|----------------|
| **DB** | RLS + `tenant_id` en cada query (ver §5.5 para pgbouncer caveat) | Bug en backend que lea cross-tenant |
| **Object storage** | Pre-signed URLs minted por control plane, scoped a `tenants/<id>/...` (R2 no soporta STS) | Worker comprometido que intente listar otros tenants |
| **Cache layer** | Redis keys SIEMPRE prefijadas: `t:<tenant_id>:k:<key>`. CDN cache-control `private` para wiki pages | Cross-tenant leak via shared cache (GAP #21) |
| **Worker container** | Container efímero per job, USER non-root (uid 65532), read-only rootfs, `--cap-drop=all` | Código del tenant intentando exfiltrar / pivotar |
| **Filesystem** | Worker monta solo `.wiki/<this-job>/` en tmpfs; código fuente en tmpfs read-only | Cross-contamination entre jobs paralelos |
| **API keys** | Inyectadas como env var al container, no logueadas, container muere al terminar | Persistencia de credenciales |
| **Egress** | Worker network namespace con egress policy: allowlist a (api.anthropic.com, api.voyageai.com, github.com, R2 endpoint, control-plane API). Implementado vía iptables en entrypoint o sidecar proxy (Fly no tiene egress firewall built-in) | Exfiltración a hosts arbitrarios |
| **Admin acceso interno** | Just-in-time access: para leer DB/S3 de un tenant, operator debe crear ticket → audit_log entry → 1h TTL | Operator rogue / forensic post-incident |

**S3/R2 scoping concreto** (corrigiendo GAP #20):

R2 NO soporta AWS STS/AssumeRole. Workflow real:
1. Worker pide a control plane: "necesito URL para `tenants/<id>/repos/<id>/wiki.tar.zst`"
2. Control plane verifica que el job es válido + tenant_id matches
3. Control plane mintea pre-signed URL con TTL = duración del job (max 10min para queries, 30min para bootstrap)
4. Worker usa esa URL para GET/PUT, no tiene credenciales R2 propias

Esto evita: worker comprometido con credenciales permanentes; bypass de tenant_id desde el worker.

### 6.2 ¿Qué pasa con el wiki como artefacto?

El wiki es Markdown + YAML. Decisión: **wiki vive en S3 como `wiki.tar.zst`** (tar comprimido). Por job:
1. Worker descarga `wiki.tar.zst` (típicamente <500KB comprimido para repos medianos)
2. Lo descomprime a `/tmp/job-<id>/.wiki/`
3. Corre `coral` con `--wiki-root /tmp/job-<id>/.wiki/`
4. Al terminar, re-comprime y sube nueva versión con object versioning de S3 (rollback gratis)

Alternativa rechazada: wiki en Postgres (`pages` table con columnas slug, body, frontmatter). Razón: queremos preservar el modelo de Coral (filesystem nativo) para no forkar.

### 6.3 Source code: ¿se almacena o se clona cada vez?

**Recomendación: clonar cada vez** (con shallow clone) para queries / ingest cortos. Para implement loops largos, cache en tmpfs del nodo con LRU.

Razones:
- Source code es grande (decenas de MB típicos, GB para monorepos)
- Almacenarlo per tenant nos pone en territorio de mirror de GitHub (riesgo legal/storage)
- GitHub no cobra clones desde GitHub Apps
- `git clone --depth=1 --branch <sha>` tarda 1-5s para repos típicos

---

## 7. GitHub App integration

### 7.1 Permisos requeridos (lo mínimo)

Para el MVP:
- **Repository permissions**:
  - `contents: read` — clonar el repo
  - `metadata: read` — listar repos/branches
- **Events** (webhooks):
  - `push` — para ingest incremental (post-MVP)
  - `pull_request` — para comentar en PRs (post-MVP)
  - `installation` — saber cuándo nos instalan/desinstalan

Para v2 (implement loop con PRs):
- `contents: write` — crear branch + commit
- `pull_requests: write` — abrir PR

### 7.2 Auth flow técnico

1. **Registro de la app**: una sola vez. Generamos `app_id` + clave privada PEM (la guardamos en KMS / Railway secrets).
2. **Por request a la API de GitHub**:
   - Firmamos un JWT con la clave privada (válido 10 min)
   - Llamamos a `POST /app/installations/<id>/access_tokens` con el JWT
   - Recibimos un **installation token** (válido 1h)
   - Usamos ese token para `git clone https://x-access-token:<token>@github.com/<full_name>.git`
3. **Cacheo de installation tokens**: en Redis con TTL = 50 min, key = `gh:installation:<id>:token`. Ahorra round-trips.

### 7.3 Webhook receiver

Endpoint `POST /api/github/webhook`. Crítico:

1. **Validar HMAC**: GitHub firma con el secret que configuramos. Si no matchea, 401.
2. **Idempotencia**: GitHub puede reentregar. Guardamos `event_id` (UUID de cada delivery) en Redis con TTL 24h; si ya lo procesamos, ignoramos.
3. **Enqueue, no procesar**: nunca procesar en el handler del webhook. GitHub timeoutea a los 10s. Solo encolar y devolver 200 inmediatamente.
4. **Dead-letter queue**: si un job falla 3 veces, va a DLQ para inspección manual.
5. **Secret rotation** (GAP #1): el webhook secret se rota cada 90 días. Procedimiento: rotamos en GitHub UI, durante 24h aceptamos ambos secrets (old + new) en el verifier, después solo el nuevo. Guardamos ambos en `secrets/github_webhook_v{N}.txt` con timestamps.
6. **Installation token cache** (GAP #2): si dos workers piden token simultáneamente y Redis miss, usamos single-flight con `SETNX` lock con TTL 30s — el primero pide, los otros esperan y leen del cache.

### 7.4 Event handlers — qué hacemos con cada evento

| Evento | Acción |
|--------|--------|
| `installation.created` | Crear `github_installations` row. Email de bienvenida. |
| `installation.deleted` | Ver §7.5 — grace period 30 días. |
| `installation.suspend` | Marcar `suspended_at`. Jobs nuevos fallan con mensaje claro. UI banner. (GAP #4) |
| `installation.unsuspend` | Limpiar `suspended_at`. |
| `installation_repositories.{added,removed}` | Sync de la lista de repos accesibles. |
| `repository.renamed` | Update `repos.full_name`. (GAP #7) |
| `repository.transferred` | Si nueva owner no tiene install: mark `disconnected`, no purgar. |
| `repository.deleted` | Mark `deleted_at`. Grace 30 días. |
| `push` (default branch) | Encolar `ingest` job si el repo está conectado y plan != free. |
| `pull_request.*` | Post-MVP: comentar en PR si afecta wiki. |

### 7.5 Lifecycle al desinstalar / borrar (GAP #3)

Cuando recibimos `installation.deleted` o `repository.deleted`:

1. Inmediato: marcar `disconnected_at` en la fila. No purgar nada.
2. Email al `tenant.email_owner`: "Tu wiki para X queda accesible 30 días más. Click acá para purgar inmediatamente."
3. UI bloquea operaciones nuevas (no más queries, no más bootstrap) pero el wiki sigue visible read-only.
4. Día 30: trigger automático borra: S3 objects (con expiration policy), DB rows, Redis cache, logs (etiquetados con tenant_id).
5. Confirmación email al owner con `delete_request_id` para audit trail.

Si el user quiere purga inmediata: endpoint `POST /api/tenants/me/purge?confirm=DELETE` — cascade descripto en §13.2.

### 7.6 GitHub App vs OAuth App — por qué App

| | GitHub App | OAuth App |
|---|------------|-----------|
| Identidad propia | Sí (`coral-bot[bot]`) | No (actúa como user) |
| Permisos por repo | Sí (instalación selectiva) | No (todo o nada) |
| Tokens cortos | Sí (1h) | No (long-lived) |
| Rate limit | 5000/h por instalación | 5000/h global por user |
| Webhooks | Built-in | Requiere setup separado |

**GitHub App es estricamente mejor para SaaS**. La única razón para OAuth es prototipo descartable.

---

## 8. Auth de usuarios

### 8.1 Login

GitHub OAuth (el mismo App o uno separado para login — recomendamos **separar**: GitHub App para acceso a repos, OAuth App para login de usuarios).

Flow:
1. `GET /auth/github` → redirect a `https://github.com/login/oauth/authorize?client_id=...&scope=read:user user:email`
2. GitHub redirige a `/auth/github/callback?code=...`
3. Cambiamos `code` por access_token
4. Llamamos a `GET /user` para obtener `github_id`, `login`, `email`
5. Upsert en `users`, creamos sesión
6. Sesión = cookie firmada con `tower-sessions` (sqlx-backed) o JWT (más simple, sin storage)

### 8.2 Autorización (RBAC)

Inicial muy simple: `owner | admin | member` por tenant. Almacenado en `tenant_members.role`.

- `owner`: billing, eliminar tenant, invitar admins
- `admin`: agregar/quitar repos, gestionar API keys, invitar miembros
- `member`: usar el wiki, hacer queries, ver historial

Middleware Axum que extrae sesión, carga tenant_id (de URL), valida membership, setea `app.tenant_id` en la transacción.

### 8.3 API tokens (para CLI futuro)

Endpoint `POST /api/tokens` para que un user genere tokens personales (ej. `corsa_pat_xxx`). Útil cuando el `coral` CLI local quiera empujar/leer del SaaS. Post-MVP.

### 8.4 CSRF & session hardening (GAP #30, #31)

**CSRF prevention** — capas:
- Cookies con `SameSite=Strict; Secure; HttpOnly; Path=/`
- Double-submit cookie: `csrf_token` también en header `X-CSRF-Token` que el frontend lee de cookie no-HttpOnly y manda en cada POST/PUT/DELETE
- Validar header `Origin` contra allowlist (`coral-saas.com`, `*.coral-saas.com`)
- Any failure → 403 con mensaje genérico

**Session fixation**:
- Regenerar session_id en login exitoso (descartar el anterior)
- TTL de sesión: 7 días absoluto + 24h sliding inactivity timeout
- Logout invalida explícitamente en DB (no solo borra cookie)

**Brute force**:
- Login rate-limit: 5 intentos / 15min por IP + 20 / hora por github_id
- Lockout no temporal (Auth via GitHub OAuth, sin password ours; el lockout solo aplica a /auth/github callback abuse)

---

## 9. Worker / job system

### 9.1 Lifecycle de un job

```
queued → running → succeeded
                ↘ failed
                ↘ cancelled
```

Estados intermedios visibles vía SSE para UI live-progress.

### 9.2 Worker concretamente

**Decisión post-review (GAP #56, #64)**: el entrypoint NO es bash. Es un binario Rust dedicado (`worker-runner`) que ya incluye: parsing del job spec, structured logging con OpenTelemetry, retry policy, output parsing tolerante (Coral no garantiza JSON en todos los subcommands), y reporting con tracing context.

**Imagen Docker hardened** (GAP #36, #38):

```dockerfile
# Builder stage
FROM rust:1.83-slim@sha256:<PINNED_DIGEST> AS builder
WORKDIR /build
COPY . .
RUN cargo build --release -p coral-cli -p worker-runner

# Runtime stage — distroless para minimal attack surface
FROM gcr.io/distroless/cc-debian12:nonroot@sha256:<PINNED_DIGEST>
COPY --from=builder /build/target/release/coral /usr/local/bin/coral
COPY --from=builder /build/target/release/worker-runner /usr/local/bin/worker-runner

# distroless/cc include /etc/ssl/certs + libgcc + libc
# Para git, montamos un volumen sidecar o usamos un base image diferente.
# Alternativa: chainguard/git static binary copiado al builder
COPY --from=alpine/git:latest@sha256:<PINNED_DIGEST> /usr/bin/git /usr/local/bin/git

# Filesystem read-only via container runtime flag (not ENV)
USER 65532:65532  # nonroot user from distroless
WORKDIR /tmp
ENTRYPOINT ["/usr/local/bin/worker-runner"]
```

Runtime flags (Fly Machine config):
```yaml
image: registry/coral-worker@sha256:<PINNED>
processes: [worker-runner]
restart: { policy: no }
machine_config:
  cpu_kind: shared
  cpus: 2
  memory_mb: 1024
  guest_security:
    read_only_root_filesystem: true
    cap_drop: [ALL]
    no_new_privileges: true
mounts:
  - source: tmpfs
    destination: /tmp
    size_gb: 4
```

**Worker token rotation (GAP #32)**: NO usamos un `WORKER_TOKEN` estático en imagen/env. En su lugar:
1. Cuando control plane crea un job, genera un JWT corto (TTL = job timeout × 1.2, max 1h) firmado con clave del control plane
2. JWT contiene: `job_id`, `tenant_id`, `repo_id`, `exp`, `aud=worker`, `scope=[fetch_spec, complete_job, mint_s3_url]`
3. Fly Machine config setea ese JWT como `JOB_TOKEN` env var al lanzar el container
4. Worker usa `JOB_TOKEN` para autenticarse contra `/api/internal/jobs/<id>/...`
5. Control plane rechaza JWT con `exp` pasado o `job_id` que ya completó
6. Token de control plane rota cada 30 días

**worker-runner pseudocode** (no bash):

```rust
async fn main() -> Result<()> {
    let job_token = env::var("JOB_TOKEN")?;
    let job_id = env::var("JOB_ID")?;
    let cp_url = env::var("CONTROL_PLANE_URL")?;

    let client = ControlPlaneClient::new(cp_url, job_token);
    let job = client.fetch_job(job_id).await?;

    let workdir = tempfile::tempdir_in("/tmp")?;
    let presigned_get = client.mint_s3_url(&job.wiki_key, Direction::Get).await?;

    // 1. Clone source
    git_shallow_clone(&job.repo_clone_url, workdir.path().join("src")).await?;

    // 2. Fetch wiki if exists
    if let Some(url) = presigned_get {
        download_and_extract(&url, workdir.path()).await?;
    }

    // 3. Secret scan pre-bootstrap (§10.4)
    if matches!(job.command, JobCommand::Bootstrap) {
        let scan = trufflehog_scan(workdir.path().join("src")).await?;
        if scan.high_confidence_findings > 0 {
            client.fail_job(job_id, "secrets_detected", scan).await?;
            return Ok(());
        }
    }

    // 4. Run coral with tracing
    let output = run_coral_with_tracing(&job, workdir.path()).await?;

    // 5. Upload wiki
    let presigned_put = client.mint_s3_url(&new_wiki_key, Direction::Put).await?;
    pack_and_upload(workdir.path().join("wiki"), &presigned_put).await?;

    // 6. Report
    client.complete_job(job_id, output, new_wiki_key).await?;
    Ok(())
}
```

### 9.3 Quotas y back-pressure

- **Token bucket por tenant**: max 10 jobs concurrentes (free), 50 (pro), 200 (team)
- **Max cost per job**: pasado como `--max-cost` al binario; si supera, exit code 2, marcamos `partial` en DB
- **Timeout por job**: bootstrap 30min, ingest 10min, query 60s, implement 10min — kill al container

### 9.4 Worker runtime — Railway long-running service (decisión v0.3)

**Decisión confirmada**: el worker es un **Railway service long-running** que consume Redis queue, no spawn per-job. Trade-off conocido vs Fly Machines:

- **(-)** Sin scale-to-zero — pagamos por el worker idle ($5-10/mo por replica)
- **(-)** Sin spawn-per-job (un container = N jobs serializados)
- **(+)** Mismo proveedor que api/web/db → ops trivial, networking interno gratis
- **(+)** DATABASE_URL/REDIS_URL inyectadas auto vía variable references
- **(+)** Costo predecible (no per-second surprise)

**Aislamiento per-job aunque la parent es long-running**:

El `worker-runner` binario en sí no ejecuta `coral` directamente — fork a un **child process** por job:

```
worker-runner (long-running)
├── loop:
│   ├── pull job from Redis (BLPOP con timeout)
│   ├── spawn child: `coral <cmd> --wiki-root /tmp/job-XXX/.wiki/ ...`
│   ├── wait child con timeout (30min bootstrap, 60s query, etc.)
│   ├── child exits → cleanup /tmp/job-XXX/
│   ├── report status to control plane
│   └── continue loop
```

Cada child tiene:
- Su propio cwd (`/tmp/job-<uuid>`)
- Su propio env (incluyendo `ANTHROPIC_API_KEY` solo de ese tenant si BYOK, o el nuestro si free tier)
- Su propio process tree (mata al child mata todo)
- Su propia conexión a la API de Anthropic (no se reusan)

Esto da ~95% del aislamiento de container-per-job sin la complejidad operativa. El 5% restante (parent process compromise) lo mitigamos con:
- Worker process re-exec cada N jobs (configurable, default 100): "graceful suicide" — termina jobs en flight, exit con código 0, Railway lo reinicia
- `cargo deny` + dependency audit estricto para evitar supply chain en el worker-runner mismo
- Memory leak monitoring (alarma si RSS > 1GB)

**Escalado**:

- **MVP (semanas 1-8)**: 1 replica worker. Suficiente para 10-50 jobs/día.
- **Post-launch (mes 2-3)**: bump a 2-3 replicas si la queue depth promedio > 5.
- **Post-PMF (>100 jobs/hora sostenidos)**: migrar workers a Fly Machines o Cloud Run jobs para verdadero spawn-per-job. La interfaz `coral` no cambia — solo el runtime wrapper.

**Job timeout enforcement**: el child process recibe SIGTERM al timeout, después SIGKILL 30s después si no respondió. Coral debería respetar SIGTERM y limpiar (verificar — `coral-cli` parece tener handlers).

---

## 10. Modelo de costos LLM

### 10.1 Decisión: BYOK desde día 1

El usuario pone su propia `ANTHROPIC_API_KEY`. Razones:

1. **Tu costo en LLM = $0**. Solo pagás infra.
2. **Pricing es trivial**: solo cobramos hosting + features.
3. **Compliance**: usuarios enterprise prefieren BYOK (data no pasa por nuestra cuenta).
4. **Sin caps de uso**: si el usuario tiene tier alto en Anthropic, lo aprovecha.

Trade-off: peor UX en el onboarding (uno debe ir a Anthropic Console, generar key, pegarla). Mitigamos con:
- Wizard de onboarding con screenshots paso a paso
- Detectar si la key es válida con un ping a `/v1/messages` con `max_tokens=1`
- Free tier: 5 queries gratis con NUESTRA key (clearly labeled), para que prueben antes de pegar la suya

### 10.2 Cost tracking

Coral ya escribe `cost_usd` por página en `.bootstrap-state.json`. Al final del job, el entrypoint del worker lo lee y lo manda al control plane → insert en `usage_ledger`.

Para queries: parseamos el output del Anthropic API (incluye `usage.input_tokens` / `output_tokens`), multiplicamos por la tarifa del modelo, registramos.

### 10.3 Pre-paid credits (opcional, post-MVP)

Si en el futuro queremos cobrar LLM ourselves (pooled API key), modelo sería:
- User compra "$50 in credits" via Stripe → suma a `tenants.credit_balance`
- Cada job debita
- Margin: ~30% sobre cost del API

Esto es más complejo (KYC, fraud, refunds) → lo dejamos para post-PMF.

### 10.4 Secret scanning pre-bootstrap (GAP #68)

El binario `coral` indexa todo el código fuente y mete excerpts en el wiki. Si el repo del user contiene secrets hardcoded (`.env`, API keys en tests, etc.), terminan en S3 con controles más débiles que los del repo original.

**Mitigación obligatoria antes del bootstrap**:

1. Worker corre `trufflehog filesystem` contra el src/ con detectors high-confidence enabled
2. Si hay matches verified (Trufflehog distingue verified vs unverified):
   - Job falla con estado `secrets_detected`
   - Email al user listando los archivos+líneas (no el secret en sí) con instrucciones para limpiar/usar git-filter-repo
   - NO se sube nada a S3
3. Si hay matches unverified pero plausibles (>0.7 confidence):
   - Job continúa pero con flag `secret_risk: true`
   - Esos archivos se EXCLUYEN del bootstrap (Coral los skip-ea)
   - Email al user con warning
4. User puede override con `tenant_settings.skip_secret_scan = true` — solo disponible vía soporte, registrado en audit_log

Trufflehog binary baked en la imagen del worker.

### 10.5 Anthropic prompt caching (GAP #67 — optimización post-MVP)

Anthropic ofrece prompt caching que reduce input token cost ~90% para contexto repetido. El wiki de Coral es ideal: las queries leen los mismos archivos repetidamente.

Implementación: worker pasa `cache_control: {type: "ephemeral"}` en el bloque de contexto del system prompt. El AnthropicApiRunner de Coral hoy no usa este flag — sería un patch upstream a `anthropic_api.rs`.

Estimación: free tier banca'o cae de ~$500/mes a ~$50-100/mes.

---

## 11. Pricing

### 11.1 Tiers propuestos

| | **Free** | **Pro** | **Team** | **Enterprise** |
|---|---|---|---|---|
| Precio | $0 | $19/mes/user | $99/mes (5 users) | Custom |
| Repos | 1 público | 5 (pub o priv) | 20 | Ilimitado |
| Bootstrap | 1/mes, max 50 archivos | 10/mes | Ilimitado | Ilimitado |
| Queries/mes | 100 (con nuestra key) | Ilimitado (BYOK) | Ilimitado (BYOK) | Ilimitado (BYOK) |
| Sync incremental | No | Sí | Sí | Sí |
| Implement loop | No | Read-only | Sí | Sí |
| Members | 1 | 1 | 5 (+$15/user extra) | Custom |
| SSO/SAML | No | No | No | Sí |
| Soporte | Community | Email | Slack shared channel | Dedicated |

### 11.2 Free tier real cost & abuse prevention (GAP #46)

Con nuestra key, 100 queries/mes = ~$3-5/mo en LLM por user gratis. Asumiendo 20% de conversión, viable hasta ~5000 free users (~$15-25k/mes en LLM bancado). Si crece más, ajustamos.

**El cálculo asume CERO abuse**, lo cual es ingenuo. Mitigaciones obligatorias:

1. **Card on file desde free tier**: Stripe Checkout en modo "setup" (sin charge inmediato). El user da una tarjeta válida para abrir cuenta free. Esto sube la fricción 1 paso pero filtra ~95% del abuse trivial.
   - Alternativa lighter: card on file requerida solo al pasar 50 queries/mes.
2. **Fingerprinting al signup**: Stripe Radar's risk_score + verificación email + check de duplicate github_id.
3. **Cap absoluto por tenant**: max `$5/mes` de LLM bancado per tenant en free, hard stop. Recovery solo via upgrade.
4. **Cap absoluto global**: budget gate. Si el LLM spend de free tier en el mes supera $X, pausamos new signups + free queries hasta el siguiente mes.
5. **Mismo billing email != múltiples tenants free**: rechazar create tenant si el `users.email` ya tiene tenant free activo.

Esto cierra los 3 vectores principales: N-account ataque (cap por email + por payment method), N-tenant ataque (cap por billing email), runaway abuse (cap absoluto).

### 11.3 Stripe webhook handling (GAP #9, #10)

Endpoint `POST /api/stripe/webhook`. Procesamos:

| Evento | Acción |
|--------|--------|
| `checkout.session.completed` | Activar plan en tenant + sync con `stripe_subscriptions` table |
| `customer.subscription.updated` | Re-sync plan + cancel_at_period_end |
| `customer.subscription.deleted` | Downgrade a free al final del periodo (no inmediato) |
| `invoice.payment_succeeded` | Marcar último pago OK + reset failure_count |
| `invoice.payment_failed` | Aplicar dunning state machine (ver §11.4) |
| `customer.deleted` | Borrar `stripe_customer_id` de tenant, mantener tenant |

**Idempotencia (GAP #10)**: tabla `stripe_events (id PK, processed_at)`. Antes de procesar, INSERT ... ON CONFLICT DO NOTHING. Si conflict, return 200 sin reprocesar.

**Validación**: Stripe firma con `Stripe-Signature` header. Validar con secret específico del webhook endpoint (rotable).

**State reconciliation job nocturno**: cron que compara `tenants.plan` con Stripe API. Si diverge (ej: webhook se perdió), corrige + alerta a Slack del equipo.

### 11.4 Tax & dunning (GAP #11, #12)

**Stripe Tax desde día 1**: 
- Habilitamos Stripe Tax en el dashboard
- Recolectamos `tax_id_collection` en Checkout
- Cobramos IVA/VAT/sales tax automáticamente según jurisdicción
- Threshold-based registration: Stripe nos avisa cuando hay que registrarse en un país

**Dunning** (revenue recovery por failed payments):
- Stripe Smart Retries habilitado (4 intentos en 21 días)
- Email al user en cada retry fail
- Día 21 sin pago → downgrade automático a free + email final
- Pro/Team pueden re-activar agregando card sin perder data

**Refund policy**:
- Pro/Team: prorated refund dentro de los primeros 14 días (alineado con EU consumer rights)
- Después de 14d: no refund pero cancel al fin de periodo
- Texto en ToS + FAQ

---

## 12. Infrastructure

### 12.1 Hosting confirmado (MVP — Railway-everywhere)

| Componente | Provider | Plan / Tier | Costo estimado |
|------------|----------|-------------|----------------|
| API (control plane) | Railway service | Hobby | $5-15/mes |
| Web (Next.js) | Railway service | Hobby | $5-15/mes |
| Worker (long-running, 1 replica) | Railway service | Hobby | $5-10/mes |
| Postgres | Railway Postgres add-on | Hobby | $5-15/mes (incluye 100h compute + 1GB) |
| Redis | Railway Redis add-on | Hobby | $5/mes |
| Object storage | Cloudflare R2 | Free hasta 10GB | $0 |
| DNS + CDN + WAF | Cloudflare | Free | $0 |
| Email transac. | Resend | Free hasta 3k/mes | $0 |
| Observability | Grafana Cloud | Free hasta 10k series | $0 |
| Status page | instatus | Free | $0 |
| Stripe | — | — | 2.9% + 30¢ per tx |
| Dominio | Namecheap/Cloudflare | — | $10/año |
| **Total infra MVP** | | | **~$30-65/mes** |

> Estos números son MUY bajos porque Railway Hobby plan da $5 de uso gratis y servicios chicos cuestan ~$5/mes cada uno. A escala (>500 usuarios activos) la cuenta cambia.

### 12.2 Trade-offs de Railway-everywhere

**Lo que ganamos**:
- Un solo dashboard, un solo billing
- Networking interno gratis entre services (api ↔ worker ↔ db)
- DATABASE_URL/REDIS_URL inyectadas automáticamente
- `railway.toml` versionado en repo (infra-as-code lite)
- Deploys via GitHub integration trivial

**Lo que perdemos vs setup más complejo**:
- Scale-to-zero del worker (Fly Machines lo daría)
- PITR de Postgres (Neon lo da; Railway tiene daily backups)
- Múltiples regiones (Railway es single-region por service en plan Hobby)
- Sin built-in load balancer entre replicas del mismo service (Railway lo hace, pero el routing es básico)

**Mitigación de los perdidos**:
- Worker idle cost: $5-10/mes — tolerable hasta que importe
- PITR: backup adicional via `pg_dump` nocturno a R2, retention 30d. Cron job en api service.
- Multi-region: post-PMF, no MVP

### 12.3 Por qué Cloudflare R2 > S3

R2 no cobra egress. Wiki reads son el caso de uso dominante (cada query baja el `.wiki/`). Con S3 esto se vuelve la mayoría del costo a escala. R2 es API-compatible.

### 12.4 Local development

`docker-compose.yml` en raíz levanta:
- `postgres:16-alpine` (puerto 5432)
- `redis:7-alpine` (puerto 6379)
- `api` (build local con `cargo run`)
- `web` (build local con `pnpm dev`)
- `worker` (build local con `cargo run`)

Mismas env vars que Railway, leídas de `.env.local` (gitignored).

---

## 13. Security & compliance

### 13.1 Threat model (resumido)

| Amenaza | Mitigación |
|---------|------------|
| Tenant A lee wiki de tenant B | RLS + S3 prefix + IAM scoping + RBAC en código |
| Worker comprometido pivotea a infra | Egress allowlist + container efímero + sin acceso a Postgres |
| API key de tenant filtrada | Encrypted at rest (KMS), nunca logueada, scoped a un job runtime |
| Webhook spoofing | Verificación HMAC obligatoria |
| Replay de webhook | Idempotencia con event_id en Redis |
| XSS en wiki rendering | Sanitizar markdown server-side (ammonia o equivalente) |
| Implementation loop genera código malicioso aplicado a repo del user | Implement = staging only en MVP; PR-based en v2 |
| Account takeover via GitHub OAuth | Pinning de github_id (no email/login que pueden cambiar) |
| Billing fraud (free tier abuse) | Rate limit por github_id + verificación de email |

### 13.2 GDPR right-to-delete cascade (GAP #16)

`POST /api/tenants/me/delete?confirm=DELETE` ejecuta el siguiente cascade. **Toda la operación es síncrona para data primaria, y registra un `delete_request_id` para audit + comunicación al user:**

1. **DB primaria (síncrono)**: DELETE de `tenants` + cascade a `users` (si era solo-owner), `repos`, `jobs`, `usage_ledger`, `tenant_secrets`, `tenant_members`, `audit_events` (excepto los etiquetados como `legal_retention` — facturas, ToS acceptances).
2. **R2 wiki objects (async, <1h)**: Listar por prefix `tenants/<id>/`, delete each + delete object versions.
3. **Redis cache (síncrono)**: Buscar keys `t:<tenant_id>:*` y borrar.
4. **Postgres backups (eventual, hasta 7d)**: aquí está la honestidad — backups retienen data borrada hasta su rotación natural. Esto se documenta en la Privacy Policy. Si user requiere purga inmediata de backups, es escalación manual (legal exception flag).
5. **Stripe customer**: detach payment methods, anonymize customer record (Stripe API permite delete pero retiene invoice history por requirement legal).
6. **Observability traces**: filter en Grafana retention policy — tenant_id-tagged traces purgan en 24h vs los 30d normales.
7. **Worker logs**: si están en Loki, query con tenant_id label y delete chunks.
8. **GitHub installation**: NO podemos forzar uninstall del app. Si user no la desinstaló, ya no le servirá (todo borrado de nuestro lado).
9. **Email final al user** con `delete_request_id`, listado de qué se borró + qué queda en backups con timeline de purga.

Endpoint expone status: `GET /api/delete-requests/<id>` muestra qué quedó completado.

### 13.3 Lo que NO necesita el MVP

- SOC 2 (post-PMF, ~$15-30k para auditoria)
- HIPAA (no aplica para devtool)
- Penetration testing externo (post-PMF)
- ISO 27001 (post-PMF)

### 13.4 Audit log al MVP (corregido desde post-PMF — GAP #35)

Tabla `audit_events`:

```sql
CREATE TABLE audit_events (
  id UUID PRIMARY KEY,
  tenant_id UUID REFERENCES tenants(id),    -- nullable si el evento es global (operator action)
  actor_user_id UUID REFERENCES users(id),  -- quien lo hizo (null si webhook/system)
  actor_type TEXT NOT NULL,                 -- 'user' | 'system' | 'operator' | 'webhook_github' | 'webhook_stripe'
  action TEXT NOT NULL,                     -- 'tenant.created', 'repo.bootstrap_started', 'wiki.exported', etc.
  resource_kind TEXT,                       -- 'repo' | 'tenant' | 'user' | 'wiki_page' | ...
  resource_id TEXT,
  metadata JSONB,                           -- {ip, user_agent, etc} — sin PII de contenido
  legal_retention BOOLEAN DEFAULT FALSE,    -- si TRUE, no se borra en GDPR delete
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX audit_events_tenant_idx ON audit_events(tenant_id, created_at DESC);
CREATE INDEX audit_events_actor_idx ON audit_events(actor_user_id, created_at DESC);
```

**Eventos auditados al MVP**:
- Auth: login, logout, session creado, fallo
- Tenancy: create, member add/remove, role change
- Repos: connect, disconnect, bootstrap_start/end, ingest, query
- Operator: cualquier acceso de admin a data de tenant (via just-in-time access flow)
- Stripe: subscribe, cancel, payment fail, plan change
- Delete requests: full audit trail

Retention: 1 año por default. Endpoint `GET /api/audit-log` para owner.

### 13.5 ToS / Privacy / Sub-processors (GAP #15, #51)

**Templates referenciados** (no escribir desde cero — usar):
- ToS: [TermsFeed](https://www.termsfeed.com/) o [iubenda](https://www.iubenda.com/) con clauses específicas:
  - IP ownership del wiki: usuario retiene IP del código; nos cede licencia limitada para generar/almacenar el wiki
  - Indemnification: usuario responde por contenido del repo (incluye DMCA + secretos)
  - Anthropic AUP pass-through (GAP #18): cláusula explícita de que el user no puede usar Coral SaaS para violar la Usage Policy de Anthropic, lista los high-level prohibitions
  - Limitation of liability: standard SaaS cap a la suma pagada en últimos 12 meses
- Privacy Policy: GDPR-compliant, lista categorías de data, sub-processors, retention, DSAR procedure
- DPA template para B2B: descargable como PDF firmable

**Sub-processors page** (`/legal/sub-processors`):

| Sub-processor | Service | Data | Location | DPA |
|---|---|---|---|---|
| Cloudflare R2 | Object storage | Wiki content, embeddings | EU + US | [link] |
| Anthropic | LLM API | Code excerpts (during queries) | US | [link] |
| Stripe | Payments | Billing data, PII | EU + US | [link] |
| Railway o Fly.io | Compute + DB | All data | US | [link] |
| Resend | Transactional email | Email addresses, content | EU | [link] |
| Grafana Cloud | Observability | Logs, traces (no PII) | EU | [link] |

Suscripción a updates: form que captura email; cuando agregamos/cambiamos sub-processor, notificamos 30 días antes (GDPR Art. 28(2)).

### 13.6 Backup & disaster recovery (GAP #23, #24)

**Targets**:
- RPO (data loss tolerable): 1 hora
- RTO (recovery time): 4 horas
- Aplicable solo a Pro/Team/Enterprise. Free: best-effort, no SLA.

**Postgres** (revisado para Railway):
- Provider: Railway Postgres add-on
- Built-in: daily backup retention 7 días (Railway Hobby) o 14 días (Pro)
- **Adicional para acercarse a PITR**: cron job nocturno en api service ejecuta `pg_dump` → R2 con retention 30 días
- **Adicional**: WAL archiving a R2 si crece la criticidad (v1)
- Backup verification mensual: descargar dump más reciente, restore a Postgres local en CI, correr query checks
- Cross-region replica: NO en MVP, ADD en v1

**R2 (wiki objects)**:
- Versioning habilitado en bucket
- Lifecycle rule: keep last 30 versions per object, delete older
- Cross-region replication a otro R2 bucket: NO en MVP

**Recovery procedure** (documented runbook):
1. Determine impact (DB? R2? specific tenant?)
2. Communicate to status page within 15min
3. Restore from PITR / R2 versions
4. Validate data integrity (run audit_events count vs expected)
5. Post-mortem dentro de 5 días

### 13.3 Logs y PII

- Logs estructurados (JSON), sin PII salvo `tenant_id` y `user_id` (UUIDs)
- API keys: NUNCA en logs (filtrar en el logging layer)
- Source code del tenant: NUNCA logueado (solo paths)
- Wiki content: NO logueado (solo metadata)
- Retention de logs: 30 días para debug, 1 año para audit (post-PMF)

---

## 14. Observability

| Señal | Tool | Útil para |
|-------|------|-----------|
| **Metrics** | Prometheus → Grafana | Jobs por minuto, latencia query p99, cost burn rate |
| **Logs** | Loki o Grafana Cloud Logs | Debug de job fallido específico |
| **Traces** | Tempo / Honeycomb (free tier) | Trace de un request end-to-end: web → enqueue → worker → response |
| **Alerts** | Grafana Alerts → Slack/PagerDuty | Job failure rate >5%, Anthropic 5xx burst, disk fill |
| **Status page** | Statuspage.io (free) | Comunicar caídas a usuarios |

Trace IDs propagados via `traceparent` header entre control plane y worker.

---

## 15. MVP phases (concretas, con criterios de aceptación)

### Fase 0 — Setup (Semana 0–1)
- [ ] Crear repo `Coral-SAAS` con estructura monorepo del §4.1 (api/, web/, worker/, shared/, docs/)
- [ ] Cargo workspace con `api`, `worker`, `shared` (frontend separado en `web/` con pnpm)
- [ ] `web/` con Next.js 15 + dependencies (next, react, tanstack-react-query, zustand, tailwind 4, @playsistemico/modo-bo-ui-lib via .npmrc + PAT)
- [ ] `railway.toml` con 3 services + Postgres add-on + Redis add-on levantados
- [ ] `docker-compose.yml` para dev local (postgres + redis + api + web + worker)
- [ ] CI: 
  - `cargo test --workspace`, `cargo clippy --all-targets -- -D warnings`, `cargo deny check`
  - `pnpm typecheck`, `pnpm lint` en web/
  - `trivy image` scan en api, web, worker dockerfiles
- [ ] Dockerfiles hardened (USER nonroot, distroless cuando aplica, pinned digests)
- [ ] **GitHub Actions con OIDC (GAP #62)**: NO usar `RAILWAY_TOKEN` estático en secrets. Railway acepta OIDC via service account dedicated. Branch protection en `main` + required reviews. Workflows con secrets requieren environment approval.

**DoD**: `curl localhost:8080/healthz` (api), `curl localhost:3000/api/health` (web) devuelven OK; worker conecta a Redis y loguea "ready"; `cargo test --workspace` + `pnpm typecheck` verdes; `trivy` sin CRITICAL en ninguna imagen.

### Fase 1 — Auth & tenant (Semana 1–2)
- [ ] GitHub OAuth login
- [ ] Sessions con cookies firmadas
- [ ] CRUD de tenants + members
- [ ] RLS configurada en Postgres
- [ ] UI básica: login → "no repos yet" placeholder

**DoD**: 2 cuentas pueden loggearse, no se ven entre sí.

### Fase 2 — GitHub App (Semana 2–3)
- [ ] App registrada en GitHub
- [ ] Install flow funcional (callback, save installation_id)
- [ ] Listar repos accesibles
- [ ] Generar installation tokens on-demand (cached)
- [ ] Webhook receiver con HMAC validation + idempotencia

**DoD**: instalo la app, veo mis repos en la UI, recibo eventos de push (logueados, no procesados).

### Fase 3 — Job system & worker (Semana 3–5)
- [ ] Job queue en Redis (apalis o equivalente)
- [ ] Worker image build + push a registry
- [ ] Fly Machines integración: spawn worker on job enqueue
- [ ] Entrypoint del worker: clone → fetch wiki → run coral → upload → report
- [ ] Status updates vía SSE al frontend

**DoD**: clic "Run bootstrap" → en <30s veo logs en vivo → al terminar veo el wiki listado.

### Fase 4 — Wiki rendering & query (Semana 5–7)
- [ ] Serve `.wiki/*.md` desde S3 con cache CDN
- [ ] Markdown rendering server-side (con sanitización)
- [ ] Navegación por slugs / backlinks
- [ ] Endpoint `/api/query` que encola job y stream-respondea
- [ ] UI de chat estilo Perplexity (pregunta → respuesta + sources)

**DoD**: bootstrap de un repo real (este Coral mismo), 5 queries reales que devuelven respuestas correctas con citas.

### Fase 5 — Polish & launch (Semana 7–8)
- [ ] Onboarding flow con wizard (time-to-first-wiki target: <8min para repo de <10k LOC)
- [ ] **Bootstrap failure UX (GAP #53)**: en `jobs` table, status `failed` con `failure_reason` enumerado:
  - `cost_cap_hit` → muestra parcial-state + botón "Continue with higher cap" + advierte BYOK already-spent
  - `repo_too_large` → bloquea antes de empezar (GAP #61, GitHub API repo.size check), sugiere Pro tier
  - `secrets_detected` → modal con archivos+líneas (no secrets), link a docs git-filter-repo
  - `parser_error` → reporta language + file que falló, fallback a "skip this file y continúa"
  - `anthropic_rate_limit` → "tu API key hit rate limit, retry en N min" (autoretry con exponential backoff)
  - `network_timeout` → retry button con visual countdown
  - Email automático al user con `failure_reason` traducido + next steps
- [ ] Wiki search/discovery (TF-IDF, no LLM cost) — GAP #40
- [ ] Staleness indicator (GAP #42): badge "wiki desfasado por N commits" con CTA "re-sync"
- [ ] Billing free-only (sin Stripe checkout pero card-on-file gate descripto en §11.2)
- [ ] Landing page (coral-saas.com)
- [ ] Privacy policy + Terms of Service + Sub-processors page publicados
- [ ] Status page (instatus free tier, GAP #28)
- [ ] Launch en Show HN / Reddit / Twitter

**DoD**: 10 usuarios externos hicieron bootstrap exitoso de su repo; al menos 1 hizo bootstrap fallido y vio el flow de error sin tickets.

---

## 16. Costos operativos estimados (primer año, v0.3)

### 16.1 MVP (0-100 usuarios totales, free-dominante)

| Item | Mensual |
|------|---------|
| Railway: api + web + worker + Postgres + Redis (Hobby) | $30-50 |
| Cloudflare R2 (wiki <10GB) | $0 |
| Anthropic (free tier banca'o: 100 users × 100 queries × ~$0.005) | $50 |
| Cloudflare DNS+CDN, Resend, Grafana, instatus | $0 |
| Dominio | $1 |
| Stripe (no charging aún) | $0 |
| **Total MVP** | **~$80-100/mes** |

### 16.2 Año 1 (500 free + 50 pro + 5 team)

| Item | Mensual | Anual |
|------|---------|-------|
| Railway: services escalados a 2x replicas en api+worker | $80-120 | $1,200 |
| Railway: Postgres bump a Pro ($20) por backup retention + más compute | $20 | $240 |
| Railway: Redis Pro ($10) | $10 | $120 |
| Cloudflare R2 (~50GB wiki + ~5GB embeddings) | $5 | $60 |
| Anthropic (free tier banca'o: 500 × 100 queries) | $250 | $3,000 |
| Anthropic prompt caching activado (post §10.5) reduce a | $50 | $600 |
| Email + observability + DNS | $30 | $360 |
| Dominio + certs | $1 | $12 |
| Stripe fees (2.9% + 30¢ × ~$5k MRR) | $150 | $1,800 |
| **OpEx total (sin prompt caching)** | **~$546/mes** | **~$6,792** |
| **OpEx total (con prompt caching)** | **~$346/mes** | **~$5,392** |
| **Revenue Pro ($19×50) + Team ($99×5)** | **~$1,445/mes** | **~$17,340** |
| **Margen bruto (sin caching)** | **~$899/mes** | **~$10,548** |
| **Margen bruto (con caching)** | **~$1,099/mes** | **~$13,548** |

Esto es **3-5x mejor** que la estimación v0.1 gracias a Railway-everywhere ($600/mes ahorrados) + prompt caching post-§10.5 ($200/mes ahorrados) + free tier abuse prevention (§11.2 card-on-file).

El negocio sigue siendo dependiente de conversión free→pro. Si la conversión es 5% (no 20%), margen cae a ~$200/mes con caching. Por eso §11.2 card-on-file gate es crítico.

---

## 17. Risks & open questions

### 17.1 Riesgos técnicos

| Riesgo | Probabilidad | Impacto | Mitigación |
|--------|--------------|---------|------------|
| Coral binary tiene bugs cuando se corre en container con paths diferentes | Media | Alto | Test exhaustivo con Docker + paths sintéticos antes de prod |
| Bootstrap de repos grandes (>100k LOC) demora >30min o cuesta >$10 | Alta | Medio | Hard cap de $5 y tamaño <50k LOC en MVP, mostrar estimación previa |
| Cloudflare R2 cambia pricing | Baja | Bajo | Capa de abstracción S3-compat, swap a B2 si pasa |
| GitHub revoca la App por TOS violation | Baja | Catastrófico | Cumplir GitHub App TOS estrictamente desde día 1 |
| Anthropic cambia API o sube precios | Media | Medio | Provider abstraction permite swap a Gemini/local |

### 17.2 Riesgos de producto

| Riesgo | Mitigación |
|--------|------------|
| GitHub Copilot Workspace + Spaces hacen lo mismo gratis | Foco en wikis legibles por humanos + queries explicables; diferenciador clave |
| Devs no quieren documentación auto-generada | User research previo a Fase 4 con 5-10 devs |
| BYOK fricciona demasiado en onboarding | Free tier con nuestra key + wizard guiado |

### 17.3 Preguntas abiertas (DECISIONES PENDIENTES)

**Decisiones cerradas en v0.3** (ya no pendientes):
- ✅ Repo: separado en `Coral-SAAS`
- ✅ Free tier: con nuestra API key
- ✅ Frontend stack: Next.js 15 + React + TanStack Query + Zustand + Tailwind + `@playsistemico/modo-bo-ui-lib`
- ✅ DB hosting: Railway Postgres add-on
- ✅ Worker hosting: Railway long-running service

**Aún pendientes**:
1. **¿GitHub App ÚNICA o separada para login vs install?** Recomendamos separar pero hay overhead operativo (GAP #8 sugiere considerar una sola app). Decisión: arrancar SEPARADAS para minimizar permisos del flow de login; revisar a los 3 meses.
2. **¿Soportamos GitLab/Bitbucket en algún momento?** No para MVP. Decidir post-PMF.
3. **¿Implement loop produce PR automáticamente o queda en staging para review humano?** Recomendamos staging-only en MVP, PR en v2.
4. **¿Damos API pública (REST) en MVP?** Recomendamos NO; primero la UI, después CLI/SDK.
5. **¿Multi-region en algún momento?** Solo si tenemos clientes europeos con datos sensibles que exigen residencia (GAP #49). Post-PMF.
6. **¿Cuánto del wiki es público?** Por default privado. ¿Permitimos "publish wiki" (read-only público con URL)?
7. **¿GitHub Marketplace listing?** GAP #66 — vale la pena el effort post-MVP si el organic install no llega por otros canales.
8. **¿Monorepos cómo se modelan?** GAP #44 — ¿un repo = un wiki, o sub-paths configurables?
9. **¿Prompt caching activado desde MVP o post-launch?** §10.5 — recomendamos POST-MVP (no critical path) pero impacta cost projection.

### 17.4 TOP 10 RISKS (síntesis del reviewer)

Ranking por probabilidad × impacto. Los gaps no resueltos están en `SAAS-PLAN-GAPS.md`; estos son los que más atención requieren durante el build:

1. **RLS + pgbouncer pool mode** (GAP #19, §5.5): cross-tenant leak silencioso si `SET LOCAL` se usa fuera de transacción explícita. **Test obligatorio en CI** que falle si algún handler no abre tx.
2. **Stripe webhook lifecycle** (GAP #9, §11.3): revenue leak desde día 1 de paid tier sin webhook reconciliation. **No hacer paid launch sin webhook handler probado**.
3. **Worker container hardening** (GAP #36, §9.2): root + writable rootFS + sin cap drop = container escape probable cuando implement loop genere código malicioso. **No-go criterion** para v2.
4. **CSRF en endpoints state-changing** (GAP #30, §8.4): bleed de LLM budget trivial sin double-submit token. **Test de pentest básico en Fase 5**.
5. **GDPR delete cascade** (GAP #16, §13.2): "delete" que deja data en backups por 7d sin documentarlo = exposición legal. **Documentar en Privacy Policy desde día 1**.
6. **Audit log al MVP** (GAP #35, §13.4): forensics imposible post-incident sin audit_events. **Movido a MVP** desde post-PMF.
7. **Worker token rotation** (GAP #32, §9.2): JWT corto per-job en lugar de bearer estático. **No-go** para MVP si seguimos con estático.
8. **Secret leakage del repo al wiki** (GAP #68, §10.4): trufflehog pre-bootstrap obligatorio. **Bloquea bootstrap si verified secrets encontrados**.
9. **GitHub Actions OIDC** (GAP #62, §15 Fase 0): no long-lived tokens en CI. **Compromiso de PR del fork = compromiso de prod sin esto**.
10. **`installation.deleted` data retention** (GAP #3, §7.5): grace period 30d + opt-in inmediato. **Documentar en ToS**.

Mid-tier risks no en top-10 pero monitorear: cold-start cost real del worker (GAP #47, probable underestimación 2-3x), free tier abuse vector (GAP #46, card-on-file mitiga), SSE behind CDN (GAP #58, separar subdomain), prompt caching opportunity (GAP #67, 5-10x cost reduction).

---

## 18. Glossary

| Término | Definición |
|---------|------------|
| **Tenant** | Cuenta de billing — puede ser un user solo o una org con miembros |
| **Repo** | Repositorio de GitHub conectado a un tenant |
| **Wiki** | El conjunto de archivos Markdown generados por Coral para un repo |
| **Bootstrap** | Generación inicial completa del wiki (caro, lento, una vez) |
| **Ingest** | Actualización incremental del wiki tras cambios al código |
| **Query** | Pregunta del user respondida por LLM con contexto del wiki |
| **Implement loop** | Pipeline UNDERSTAND→PLAN→VERIFY→GENERATE→VALIDATE→STAGE de Coral |
| **BYOK** | Bring Your Own Key — user provee su API key de Anthropic |
| **Control plane** | El servicio web que orquesta y sirve UI |
| **Worker** | Container efímero que ejecuta el binario `coral` para un job |
| **RLS** | Row-Level Security de PostgreSQL |

---

## 19. Next steps después de aprobar este plan

1. Crear repo `Coral-SAAS` en GitHub con README mínimo
2. Setup Cargo workspace + CI básico
3. Decidir: ¿Railway o Fly para control plane?
4. Comprar dominio (`coral-saas.com` o similar)
5. Empezar Fase 0
