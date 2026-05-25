# Coral SaaS

Multi-tenant SaaS wrapper around [Coral](https://github.com/agustincbajo/Coral) — hosted code-wiki + LLM queries + implement loop, billed monthly.

> Read [docs/SAAS-PLAN.md](docs/SAAS-PLAN.md) first. It's the source of truth for architecture, data model, GitHub App integration, billing, security model, and MVP phasing.
> [docs/SAAS-PLAN-GAPS.md](docs/SAAS-PLAN-GAPS.md) tracks the 70 production-readiness gaps identified by reviewers.

## Status

`v0.1.0` — scaffold only. Nothing implemented yet beyond `/healthz` stubs. See SAAS-PLAN.md §15 "MVP phases" for the build sequence.

## Stack

- **API** (`api/`) — Rust + Axum + sqlx, control plane (auth, GitHub App, Stripe, job enqueue)
- **Worker** (`worker/`) — Rust binary, consumes Redis queue, spawns `coral` subprocess per job
- **Web** (`web/`) — Next.js 15 + React + TanStack Query + Zustand + Tailwind 4 + [`@playsistemico/modo-bo-ui-lib`](https://github.com/playsistemico/modo-bo-ui-lib)
- **Shared** (`shared/`) — Rust crate with cross-service types (`JobSpec`, `JobResult`, errors)
- **Hosting**: Railway (api + web + worker services, Postgres + Redis add-ons), Cloudflare R2 for object storage

## Local dev

Prerequisites: Docker, Rust 1.83+, Node 20+, pnpm 9+, a GitHub PAT with `read:packages` scope (for `modo-bo-ui-lib`).

```bash
# 1. Setup env
cp .env.example .env.local  # then fill in secrets
export GITHUB_PACKAGES_TOKEN=ghp_...  # for modo-bo-ui-lib

# 2. Bring up the stack
docker compose up --build

# 3. Hit the services
curl http://localhost:8080/healthz       # api
curl http://localhost:3000/api/health    # web
docker compose logs worker               # worker logs
```

For iterative Rust dev without rebuilding Docker:

```bash
# Terminal 1
docker compose up postgres redis

# Terminal 2 — api
cargo run -p api

# Terminal 3 — worker
cargo run -p worker

# Terminal 4 — web
cd web && pnpm install && pnpm dev
```

## Deploy

Auto-deploy to Railway on `main` push via `.github/workflows/deploy.yml` (OIDC, no static tokens).

To deploy manually:

```bash
railway up --service api
railway up --service web
railway up --service worker
```

## Repository layout

```
.
├── api/             # Rust control plane (Axum)
├── worker/          # Rust queue consumer + coral subprocess runner
├── shared/          # Rust crate, types shared across api + worker
├── web/             # Next.js 15 frontend
├── migrations/      # sqlx migrations (numbered .sql)
├── docs/            # SAAS-PLAN.md + SAAS-PLAN-GAPS.md
├── .github/workflows/  # CI + deploy
├── docker-compose.yml  # Local dev
├── railway.toml        # Railway services config
└── Cargo.toml          # Rust workspace
```

## License

UNLICENSED — internal project.
