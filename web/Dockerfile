FROM node:24.21.0-bookworm-slim AS build
WORKDIR /web
RUN corepack enable && corepack prepare pnpm@12.11.2 --activate
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
RUN pnpm install --frozen-lockfile
COPY . .
RUN pnpm build
FROM caddy:2.11.7-alpine
COPY --from=build /web/build /srv
