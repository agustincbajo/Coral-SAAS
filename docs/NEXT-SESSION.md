# NEXT-SESSION.md — Trabajo pendiente para la siguiente sesión

> Este documento es **autosuficiente**: una sesión nueva de Claude (o cualquier dev) puede arrancar desde cero leyéndolo, sin tener contexto previo.
>
> Lee primero, en orden:
> 1. `README.md` — orientación
> 2. `docs/SAAS-PLAN.md` (v0.3) — arquitectura objetivo
> 3. `docs/STATUS.md` — qué está hecho hoy
> 4. `docs/SAAS-PLAN-GAPS.md` — 70 gaps de producción identificados
> 5. Este archivo — qué hacer ahora

**Última actualización**: fin de sesión autónoma 2026-05-25.
**Estado del repo**: 9 commits, `cargo check --workspace` limpio, 4/4 tests pasan, todo pusheado a `main`.

---

## Cómo está organizado este doc

Tres tracks paralelos. Dentro de cada track, items ordenados por prioridad (P0 > P1 > P2):

- **Track A — Critical path para deploy-able MVP** (sin esto no se puede mostrar a nadie)
- **Track B — Features que agregan valor visible**
- **Track C — Hardening, ops y gaps de producción**

Cada item tiene:
- 🎯 **Qué**: objetivo concreto
- 🤔 **Por qué**: contexto + por qué importa
- 📍 **Dónde**: archivos/líneas a tocar
- ⏱ **Esfuerzo**: estimación realista
- 🔗 **Referencias**: SAAS-PLAN/GAPS

---

## Track A — Critical path para deploy MVP

### A1 (P0) — Setup manual de Railway + GitHub Apps

🎯 **Qué**: configurar todas las cuentas externas y secrets para que el código corra.

🤔 **Por qué**: el código está listo, pero sin Railway services + GitHub App + secrets no arranca nada. Esto es bloqueante y NO autónomo (requiere clicks del usuario en consolas web).

📍 **Pasos concretos**:

1. **Railway** (`railway.app`):
   - Crear proyecto nuevo, conectar a `github.com/agustincbajo/Coral-SAAS`
   - Railway detectará el `railway.toml` y proponer crear 3 services (`api`, `web`, `worker`)
   - Agregar el **Postgres add-on** → expone `DATABASE_URL` automáticamente como variable de referencia
   - Agregar el **Redis add-on** → expone `REDIS_URL`
   - Configurar las env vars del `.env.example` (todas las que faltan) en el dashboard de Railway
   - **Importante**: `DATABASE_URL` y `REDIS_URL` deben ser **shared** entre los 3 services
2. **GitHub OAuth App** (`github.com/settings/developers` → New OAuth App):
   - Application name: `Coral`
   - Homepage URL: `https://<tu-dominio>` (o el de Railway)
   - Authorization callback URL: `https://<tu-dominio>/auth/github/callback`
   - Generar Client ID + Client Secret → guardar en Railway como `GITHUB_OAUTH_CLIENT_ID` / `GITHUB_OAUTH_CLIENT_SECRET`
3. **GitHub App** (`github.com/settings/apps/new` — distinta de OAuth):
   - Name: `Coral SaaS` (debe ser único globalmente)
   - Homepage URL: `https://<tu-dominio>`
   - Setup URL: `https://<tu-dominio>/api/github/install/callback`
   - Webhook URL: `https://<tu-dominio>/api/github/webhook`
   - Webhook secret: generar 32 bytes random hex → guardar como `GITHUB_WEBHOOK_SECRET`
   - Permissions (Repository): `contents: read`, `metadata: read`
   - Subscribe to events: `installation`, `installation_repositories`, `repository`, `push`
   - Generar private key (.pem) → guardar contenido en Railway como `GITHUB_APP_PRIVATE_KEY` (reemplazar `\n` literal por `\n` si Railway lo pide)
   - Anotar el App ID → `GITHUB_APP_ID`
4. **Cloudflare R2**:
   - Crear bucket `coral-saas-wiki` (o el que prefieras)
   - Generar API Token con permisos R/W al bucket
   - Anotar `R2_ENDPOINT` (algo como `https://<account>.r2.cloudflarestorage.com`), `R2_ACCESS_KEY_ID`, `R2_SECRET_ACCESS_KEY`
5. **Stripe** (test mode primero):
   - Crear cuenta Stripe, obtener `STRIPE_SECRET_KEY` (sk_test_...)
   - Configurar webhook → endpoint `https://<tu-dominio>/api/stripe/webhook`
   - Guardar el webhook signing secret como `STRIPE_WEBHOOK_SECRET`
   - (post-MVP) Habilitar Stripe Tax
6. **GitHub Packages PAT** (`github.com/settings/tokens` → fine-grained):
   - Scope: `read:packages`
   - Guardar como secret `GITHUB_PACKAGES_TOKEN` en:
     - GitHub Actions repo settings (para que CI haga `pnpm install`)
     - Railway env (para que el build del web image pueda hacer `pnpm install`)
7. **Resend** (opcional para MVP):
   - `RESEND_API_KEY` para emails transaccionales

⏱ **Esfuerzo**: 1-2 horas (la mayoría es esperar redirects y copiar/pegar secrets)

🔗 **Referencias**: `.env.example`, `SAAS-PLAN.md` §7, §15 Fase 0

---

### A2 (P0) — Vendoreo del binario Coral en el worker

🎯 **Qué**: que el worker arranque con el binario `coral` disponible en `/usr/local/bin/coral`.

🤔 **Por qué**: el `worker/Dockerfile` ya tiene el pattern de download desde release de GitHub, pero la URL apunta a `github.com/agustincbajo/Coral/releases/download/v0.41.0/coral-v0.41.0-x86_64-unknown-linux-gnu.tar.gz` — esa release puede no existir.

📍 **Dónde**: `worker/Dockerfile`, líneas con `ARG CORAL_VERSION` y `RUN curl ...`

📋 **Opciones**:

- **Opción 1 (rápida)**: crear una release manual en `github.com/agustincbajo/Coral/releases` con un tarball que contenga el binario `coral` compilado para Linux x86_64. Ajustar `CORAL_VERSION` en el Dockerfile.
- **Opción 2 (limpia)**: setear un workflow en el repo de Coral que publique releases con binarios cross-compilados (`x86_64-unknown-linux-gnu`, `aarch64`, etc.). Después referenciar desde acá.
- **Opción 3 (vendoreo local)**: agregar el binario al repo Coral-SAAS bajo `worker/vendor/coral` y copiarlo en el Dockerfile. Solo si la release pública no es posible.

⏱ **Esfuerzo**: 30min-2h dependiendo de la opción

🔗 **Referencias**: `worker/Dockerfile`, `SAAS-PLAN.md` §9.2, `STATUS.md` Track Jobs & Worker

---

### A3 (P0) — Flipear `MOCK_MODE = false` y wirear el subprocess real

🎯 **Qué**: que el worker ejecute `coral` de verdad en lugar de simular trabajo con un `sleep(2s)`.

🤔 **Por qué**: hoy todo job termina con un output falso. Sin esto, ningún wiki se genera nunca.

📍 **Dónde**: `worker/src/coral_runner.rs`

📋 **Implementar el path documentado en los comentarios del archivo** (8 pasos):

1. Crear `/tmp/job-<id>` y `chdir`.
2. Pedir pre-signed URL para el clone (si fuera necesario). Para MVP: usar `git clone https://x-access-token:<inst_token>@github.com/<full_name>.git`. El installation token se obtiene llamando al API:

   ```rust
   // Pseudocódigo del API call que necesitamos agregar al worker
   let inst_token = get_installation_token(spec.installation_id).await?;
   let clone_url = format!(
       "https://x-access-token:{}@github.com/{}.git",
       inst_token, spec.repo_full_name
   );
   ```

   ⚠️ Esto requiere que el worker tenga acceso a un endpoint que mintee tokens, O que el `JobSpec` ya incluya el token (riesgo: queda en Redis con TTL). Decisión recomendada: ampliar `JobSpec` con `installation_token: Option<String>` minted-at-enqueue por el `api`, válido por job timeout × 1.2.
3. (Si `wiki_get_url`) Descargar + extraer `wiki.tar.zst` con `tar -xzf` (zstd).
4. Si bootstrap: correr `trufflehog filesystem --json src/` y abortar con `failure_reason = "secrets_detected"` si hay matches verified (cierra GAP #68).
5. Spawn `coral <cmd> --wiki-root .wiki/ --provider anthropic_api --max-cost <N> --json`. La env var `ANTHROPIC_API_KEY` se inyecta solo aquí.
6. Parse stdout. Coral no garantiza JSON en todos los subcommands; fallback a leer `.coral/.bootstrap-state.json` para cost/tokens.
7. `tar --use-compress-program=zstd -cf wiki.tar.zst .wiki/`. Subir vía `wiki_put_url` (pre-signed PUT minted por el api al enqueue).
8. Devolver `JobResult` con `cost_usd`, `input_tokens`, `output_tokens` parseados del state file.

⏱ **Esfuerzo**: 4-6h (incluyendo el endpoint `api/src/routes/internal_jobs.rs` para mint de installation tokens al worker, y testing manual con un repo real)

🔗 **Referencias**: `worker/src/coral_runner.rs`, `SAAS-PLAN.md` §9.2

---

### A4 (P0) — Mint de pre-signed URLs al enqueue

🎯 **Qué**: cuando el `api` encola un job, debe incluir `wiki_get_url` (si existe wiki previo) y `wiki_put_url` (TTL ≥ job timeout × 1.2) en el `JobSpec`.

🤔 **Por qué**: el worker hoy no tiene acceso a R2 — y no debería tenerlo (GAP #20 — no permanent creds). El path correcto es: api mintea URLs pre-signed al enqueue.

📍 **Dónde**:

- `api/src/routes/jobs.rs::start_bootstrap` — antes del `enqueue()`, llamar a `r2::presigned_get(...)` y `r2::presigned_put(...)` con TTL adecuado.
- `api/src/r2/mod.rs` ya tiene las funciones; solo hace falta usarlas.
- TTLs: bootstrap 30min × 1.2 = 36min para PUT. GET TTL = 10min basta.
- Setear `spec.wiki_put_url = Some(presigned_put_url)` antes de pasar al `enqueue`.

⏱ **Esfuerzo**: 1h (más cobertura de test)

🔗 **Referencias**: `api/src/routes/jobs.rs`, `api/src/r2/mod.rs`, `SAAS-PLAN.md` §6.1

---

### A5 (P1) — Worker → API callbacks vía JWT corto (en lugar de DB directo)

🎯 **Qué**: que el worker no escriba directamente a Postgres. En vez, callee a `/api/internal/jobs/:id/complete` con un JWT corto firmado per-job.

🤔 **Por qué**: el SAAS-PLAN §9.2 dice esto explícitamente. Hoy el worker tiene `DATABASE_URL` y escribe directo — funciona en Railway internal network pero es defensa-en-profundidad pobre y bloquea migración a Fly Machines.

📍 **Dónde**:

- Nueva ruta `api/src/routes/internal_jobs.rs` con auth middleware que valide JWT firmado con `WORKER_JWT_SECRET`.
- `api/src/jobs/mod.rs::enqueue` debe mintar el JWT (claims: `job_id`, `tenant_id`, `exp = now + timeout × 1.2`, `aud = "worker"`) y incluirlo en el `JobSpec`.
- `shared/src/lib.rs::JobSpec` agregar `pub job_token: String`.
- `worker/src/main.rs` reemplazar las queries directas por calls al API con bearer `job_token`.

⏱ **Esfuerzo**: 4-6h

🔗 **Referencias**: `SAAS-PLAN.md` §9.2, GAP #32

---

### A6 (P0) — Wiki tarball extraction al subir desde el worker

🎯 **Qué**: cuando el worker sube `wiki.tar.zst` a R2, descomprimir y guardar cada `.md` individual en la key esperada por el render endpoint.

🤔 **Por qué**: el endpoint `routes/wiki.rs` espera objetos en `tenants/<id>/repos/<id>/wiki/<slug>.md`, pero el worker hoy subiría un único tarball.

📋 **Opciones**:

- **Opción 1 (worker-side)**: el worker, antes de subir, hace `tar --extract` y sube cada archivo individualmente. Más calls a R2 pero más simple.
- **Opción 2 (api-side)**: subir el tarball, agregar un endpoint que extraiga on-demand (costoso). NO recomendado.
- **Opción 3 (lazy)**: el endpoint `routes/wiki.rs` baja el tarball y extrae en memoria (cacheable). Funciona pero agrega latencia por request.

Recomendación: **Opción 1** — el worker sube `wiki/<slug>.md` por cada archivo + un `wiki/_index.tar.zst` para backups/portabilidad.

📍 **Dónde**: `worker/src/coral_runner.rs` paso 7

⏱ **Esfuerzo**: 2h

🔗 **Referencias**: `api/src/routes/wiki.rs`, `SAAS-PLAN.md` §6.2

---

## Track B — Features que agregan valor visible

### B1 (P1) — Query endpoint con SSE (chat con el LLM sobre el wiki)

🎯 **Qué**: endpoint `POST /api/tenants/:t/repos/:r/query` que encole un job tipo `JobKind::Query`, y endpoint `GET /api/jobs/:id/stream` (SSE) que stree-eee status + final result.

🤔 **Por qué**: es la feature visible más impactante de Coral — el "Ask anything about your codebase" estilo Perplexity. Sin esto no hay demo wow.

📍 **Dónde**:

- Nuevo handler `api/src/routes/query.rs`
- SSE endpoint nuevo `api/src/routes/job_stream.rs` (usar `axum::response::Sse`)
- Worker: agregar dispatch en `coral_runner.rs` que corra `coral query "<pregunta>" --json`
- Frontend: nueva página `web/src/app/(dashboard)/repos/[id]/query/page.tsx` con UI de chat

⏱ **Esfuerzo**: 1-2 días (incluyendo el frontend de chat)

🔗 **Referencias**: `SAAS-PLAN.md` §3.2 Flow B

⚠️ **Caveat**: SSE detrás de Cloudflare free tier muere a los 100s (GAP #58). Si el query toma más, va a romper la conexión. Solución: separar SSE en un subdomain sin CDN, o usar long-polling como fallback.

---

### B2 (P1) — Wiki search/discovery (TF-IDF, no LLM)

🎯 **Qué**: endpoint `GET /api/tenants/:t/repos/:r/wiki/search?q=...` que busque en los slugs + títulos + cuerpo de las páginas del wiki.

🤔 **Por qué**: hoy el único modo de encontrar una página es por slug exacto. Searching cuesta 0 LLM si es TF-IDF (GAP #40).

📍 **Dónde**:

- Nuevo `api/src/routes/wiki_search.rs`
- Index: build on-the-fly al primer query (~ms para repos chicos), o pre-built en el worker post-bootstrap y subido como `wiki/_search-index.json` en R2.

⏱ **Esfuerzo**: 4-6h

🔗 **Referencias**: GAP #40

---

### B3 (P1) — Stripe checkout endpoint + upgrade UX

🎯 **Qué**: endpoint `POST /api/tenants/:t/billing/checkout` que devuelva una Stripe Checkout Session URL. Frontend: botón "Upgrade to Pro" en `/dashboard/settings`.

🤔 **Por qué**: sin esto no hay revenue path. Hoy un usuario que quiere pasar a Pro no tiene cómo.

📍 **Dónde**:

- Nuevo `api/src/routes/billing.rs`
- Llamar `stripe::checkout::Session::create` con `client_reference_id = tenant_id.to_string()` (necesario para que el webhook `checkout.session.completed` pueda linkear).
- Setear `success_url` y `cancel_url` apuntando al dashboard.
- Frontend: página `web/src/app/(dashboard)/settings/billing/page.tsx`.

⏱ **Esfuerzo**: 1 día

🔗 **Referencias**: `SAAS-PLAN.md` §11.3, GAP #9

---

### B4 (P1) — Polish del frontend con componentes reales de `modo-bo-ui-lib`

🎯 **Qué**: reemplazar los elementos plain-Tailwind por componentes de `@playsistemico/modo-bo-ui-lib` (`Button`, `Card`, `Table`, `Sidebar`, etc.).

🤔 **Por qué**: hoy se ve genérico. El propósito de elegir modo-bo-ui-lib era usarlo.

📍 **Dónde**:

- `web/src/components/sidebar.tsx` → usar `Sidebar` + `SidebarNavigation` de modo-bo-ui-lib
- `web/src/components/topbar.tsx` → usar `PageHeader` o equivalente
- `web/src/app/(dashboard)/repos/page.tsx` → usar `Table` + `Button` + `Tag` (para status badges)
- `web/src/app/login/page.tsx` → usar `Button` con variant `primary` + `Card`

⚠️ Requiere: tener `GITHUB_PACKAGES_TOKEN` configurado y poder hacer `pnpm install` localmente. Sin eso no se puede ver el resultado.

⏱ **Esfuerzo**: 1-2 días

🔗 **Referencias**: `/Users/agustinbajo/Downloads/modo-bo-ui-lib/src/components/` para ver el catálogo completo, y `/Users/agustinbajo/Documents/GitHub/em-dashboard/web/src/components/` para patrones de uso reales.

---

### B5 (P2) — Página de settings (tenant + miembros + API keys BYOK)

🎯 **Qué**: pantallas en `/dashboard/settings` para:
- Editar nombre del tenant
- Invitar miembros (vía github_login)
- Pegar API key de Anthropic (BYOK) — encriptada at-rest con KMS

🤔 **Por qué**: la app no tiene gestión de cuenta más allá de "logout".

📍 **Dónde**:

- Backend: nuevos endpoints en `api/src/routes/settings.rs` (membership CRUD ya tiene los modelos pero no la ruta)
- BYOK: necesita `tenant_secrets` table (existe) + KMS integration (no implementada). Para MVP rápido: usar `WORKER_JWT_SECRET` como AES-256 key, encriptar y guardar en BYTEA. Documentar como tech debt — KMS de verdad post-PMF.
- Frontend: `web/src/app/(dashboard)/settings/` con sub-páginas

⏱ **Esfuerzo**: 1-2 días

🔗 **Referencias**: `SAAS-PLAN.md` §5.4, GAP #33

---

## Track C — Hardening, ops y gaps de producción

### C1 (P1) — Tests de integración con testcontainers

🎯 **Qué**: tests que arranquen un Postgres efímero, corran las migrations, ejecuten requests contra el `api`, validen state en DB.

🤔 **Por qué**: hoy hay 4 unit tests. Cero cobertura de la integración (auth flow, RLS, webhook handlers).

📍 **Dónde**:

- `api/tests/integration.rs` (nuevo)
- Dep: `testcontainers = "0.23"` + `testcontainers-modules`
- Helpers para crear tenant + user + session + auth header

⏱ **Esfuerzo**: 2-3 días para una suite mínima sólida (auth, RLS isolation, webhook idempotency)

🔗 **Referencias**: `SAAS-PLAN.md` §15 Fase 0 DoD

⚠️ **Bloqueante**: necesitás Docker disponible. Si seguís sin Docker, ignorar este track hasta entonces.

---

### C2 (P1) — Webhook secret rotation (GAP #1)

🎯 **Qué**: soportar dos webhook secrets activos al mismo tiempo durante una rotation window.

📍 **Dónde**: `api/src/github_app/webhook.rs::verify_signature` debería probar contra `WEBHOOK_SECRET` y `WEBHOOK_SECRET_PREV` si el segundo está set.

⏱ **Esfuerzo**: 1-2h

🔗 **Referencias**: GAP #1

---

### C3 (P1) — Permission upgrade re-consent flow (GAP #5)

🎯 **Qué**: cuando agreguemos permisos al GitHub App (necesario para v2 — implement loop con PRs), las instalaciones existentes deben re-consentir.

📍 **Dónde**:

- Tabla `github_installations` agregar `permissions_version INT NOT NULL DEFAULT 1`
- Frontend: banner cuando `permissions_version < CURRENT_PERMISSIONS_VERSION` con CTA "Re-authorize"
- API endpoint `GET /api/github/install/re-consent` que redirige a GitHub para reinstall

⏱ **Esfuerzo**: 1 día

🔗 **Referencias**: GAP #5

---

### C4 (P2) — OpenTelemetry export wiring

🎯 **Qué**: que los traces lleguen a Grafana Cloud (o equivalente).

🤔 **Por qué**: las libs están en `Cargo.toml` (`tracing-opentelemetry`, `opentelemetry-otlp`) pero el `Subscriber` en `main.rs` solo tiene `fmt::layer().json()`. No se exporta nada.

📍 **Dónde**:

- `api/src/main.rs` agregar `opentelemetry_otlp::new_pipeline().tonic().with_endpoint(...).install_batch(...)?`
- Variable: `OTEL_EXPORTER_OTLP_ENDPOINT`, `OTEL_EXPORTER_OTLP_HEADERS`
- Replicar en `worker/src/main.rs`

⏱ **Esfuerzo**: 2-3h

🔗 **Referencias**: `SAAS-PLAN.md` §14

---

### C5 (P2) — Graceful shutdown

🎯 **Qué**: que SIGTERM (Railway al hacer redeploy) deje terminar requests en flight + drenar la cola del worker antes de exit.

📍 **Dónde**:

- `api/src/main.rs` agregar `axum::serve(...).with_graceful_shutdown(shutdown_signal()).await?`
- `worker/src/main.rs` agregar select! entre BLPOP y signal_hook

⏱ **Esfuerzo**: 2h

🔗 **Referencias**: estándar Tokio

---

### C6 (P2) — GDPR delete cascade implementado (GAP #16)

🎯 **Qué**: endpoint `POST /api/tenants/me/delete?confirm=DELETE` con el cascade del SAAS-PLAN §13.2.

🤔 **Por qué**: el plan promete delete, hoy no existe.

📍 **Dónde**: nuevo `api/src/routes/account_delete.rs` con la lógica:

1. DELETE de tenant + cascade
2. Listar + delete R2 objects con prefix `tenants/<id>/`
3. Limpiar Redis keys `t:<id>:*`
4. Email al owner con `delete_request_id`

⏱ **Esfuerzo**: 4-6h

🔗 **Referencias**: GAP #16

---

### C7 (P2) — Audit access endpoint para el tenant owner

🎯 **Qué**: `GET /api/tenants/me/audit-log` que pagine los `audit_events` del tenant.

🤔 **Por qué**: la tabla existe pero no hay UI para consultarla. El SOC2-grade audit requiere que el owner pueda ver su propio log.

📍 **Dónde**: nuevo `api/src/routes/audit.rs` + UI `web/src/app/(dashboard)/settings/audit/page.tsx`

⏱ **Esfuerzo**: 1 día

🔗 **Referencias**: `SAAS-PLAN.md` §13.4

---

### C8 (P2) — Sub-processors page pública

🎯 **Qué**: `/legal/sub-processors` (Next.js page) con la tabla del SAAS-PLAN §13.5.

⏱ **Esfuerzo**: 30min

🔗 **Referencias**: GAP #51

---

### C9 (P2) — Limpiar warnings de dead_code

🎯 **Qué**: remover `#![allow(dead_code)]` en `api/src/main.rs` línea 6. Marcar cada caso específico con `#[allow(dead_code)]` donde corresponda, o eliminar lo verdaderamente muerto.

⏱ **Esfuerzo**: 1-2h

🔗 **Referencias**: `CLAUDE.md` "Tech-debt" section

---

### C10 (P2) — sqlx prepare metadata + `query_as!` macros

🎯 **Qué**: convertir todas las queries de runtime-typed (`sqlx::query_as::<_, T>("SQL")`) a compile-checked (`query_as!(T, "SQL")`).

📋 **Requisito**: tener Docker para spin up Postgres, correr `cargo sqlx prepare`, commit del `.sqlx/` cache, y setear `SQLX_OFFLINE=true` en CI.

⏱ **Esfuerzo**: 1 día (más bug-fixing de typos en SQL que `query_as!` detectará)

🔗 **Referencias**: `CLAUDE.md` "Tech-debt" section

---

## Orden recomendado para retomar

Si la siguiente sesión tiene **3-4 horas**:
1. A1 — setup manual (1-2h, no autónomo, pero bloqueante)
2. A2 — vendoreo binario coral (30min-1h)
3. A4 — pre-signed URLs al enqueue (1h)

Si la siguiente sesión tiene **un día completo**:
1. A1 + A2 + A4 (mañana)
2. A3 — worker subprocess real (tarde)
3. Smoke test del flow completo

Si la siguiente sesión tiene **una semana**:
1. Día 1: A1-A6 (critical path completo) → MVP funcional
2. Día 2-3: B1 (query SSE) + B3 (Stripe checkout)
3. Día 4: B4 (polish frontend con modo-bo-ui-lib)
4. Día 5: C1 (tests de integración) + C2-C3 (security gaps)

---

## Lo que NO recomiendo hacer todavía

- **B5 settings BYOK** sin KMS real — encriptar con un secret del .env es teatro de seguridad
- **C1 tests integración** sin Docker disponible — frustrante
- **Multi-region**, **SOC2**, **SAML SSO** — post-PMF, ni los toques ahora
- **GitLab/Bitbucket support** — definir post-PMF si hay demand
- **Implement loop con PRs** (Coral v2) — requiere C3 (re-consent flow) primero, y bastante UX work

---

## Pregunta antes de retomar

Mandate a la nueva sesión: **"Lee `docs/NEXT-SESSION.md` y `docs/STATUS.md`. Decime qué track querés que avance primero."**

Eso evita que la sesión gaste contexto re-descubriendo el estado.
