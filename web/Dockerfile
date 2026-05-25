# syntax=docker/dockerfile:1.7

# ---- Dependencies ----
FROM node:20-alpine AS deps
WORKDIR /app
RUN corepack enable
COPY web/package.json web/.npmrc ./
# GITHUB_PACKAGES_TOKEN must be passed as a build arg / Railway shared
# variable so pnpm can fetch @playsistemico/modo-bo-ui-lib.
ARG GITHUB_PACKAGES_TOKEN
ENV GITHUB_PACKAGES_TOKEN=${GITHUB_PACKAGES_TOKEN}
RUN --mount=type=cache,target=/root/.local/share/pnpm/store \
    pnpm install --frozen-lockfile || pnpm install

# ---- Builder ----
FROM node:20-alpine AS builder
WORKDIR /app
RUN corepack enable
COPY --from=deps /app/node_modules ./node_modules
COPY web/ ./
ENV NEXT_TELEMETRY_DISABLED=1
RUN pnpm build

# ---- Runtime ----
FROM node:20-alpine AS runtime
WORKDIR /app
RUN addgroup --system --gid 65532 nextjs \
    && adduser --system --uid 65532 nextjs

# Next.js standalone output already includes the trimmed node_modules.
COPY --from=builder --chown=nextjs:nextjs /app/.next/standalone ./
COPY --from=builder --chown=nextjs:nextjs /app/.next/static ./.next/static
COPY --from=builder --chown=nextjs:nextjs /app/public ./public

USER 65532:65532
ENV PORT=3000
ENV NODE_ENV=production
ENV NEXT_TELEMETRY_DISABLED=1
EXPOSE 3000
CMD ["node", "server.js"]
