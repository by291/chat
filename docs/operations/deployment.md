# Single-host HTTPS deployment

Phase one uses Docker Compose on one host without high availability or cross-host
disaster recovery. API, worker, PostgreSQL, Valkey, Centrifugo, and SeaweedFS remain
on an internal network. Caddy alone publishes ports 80/443. SvelteKit builds a static
SPA, so production needs no resident Node server.

Images are pinned to Rust 1.99.0, Node 24.21.0, Caddy 2.11.7, PostgreSQL 18.6,
Valkey 9.1.2, Centrifugo 6.9.7, and SeaweedFS 4.48. The selected Caddy version is
documented in its [release notes](https://github.com/caddyserver/caddy/releases/tag/v2.11.7).
Cargo and pnpm lockfiles pin application dependencies.

Use the development initializer to generate random secrets, copy `.env` to
`.env.production`, and configure the deployment:

```dotenv
CHAT_DOMAIN=chat.example.com
CHAT_ORIGIN=https://chat.example.com
CHAT_SUBNET=172.30.240.0/24
CHAT_CADDY_IP=172.30.240.10
```

Resolve the domain to the host and allow inbound 80/443. Compose replaces development
database/Valkey URLs, listener addresses, and cookie settings. Choose a subnet that
does not overlap another network and put the Caddy address inside it. The API trusts
only that explicitly configured proxy address. Caddy replaces the forwarded client
address, so an external `X-Forwarded-For` cannot bypass per-client limits.

```powershell
pnpm -C web api:check
docker compose --env-file .env.production --file infra/compose/compose.prod.yaml up -d --build --wait
docker compose --env-file .env.production --file infra/compose/compose.prod.yaml exec api admin bootstrap --username alice --display-name Alice
docker compose --env-file .env.production --file infra/compose/compose.prod.yaml exec api admin invite
```

Open `CHAT_ORIGIN`. The production cookie is `__Host-chat_session`, with Secure,
HttpOnly, SameSite=Lax, Path=/, and no Domain. Caddy acquires/renews certificates and
stores them in `caddy-data`. Deep links return the static SPA fallback. API and
WebSocket routes have explicit upstreams; `/internal/*`, `/metrics`, and Centrifugo
`/api/*` return 404 at the public entry point.

```powershell
docker compose --env-file .env.production --file infra/compose/compose.prod.yaml logs --tail 100 api worker
```

API responses include `X-Request-ID`; error JSON and tracing logs use the same ID.
Logs contain route templates, status, and duration, without bodies, passwords,
cookies, invitation tokens, or query strings. Private API/worker metrics use
8081/metrics and 8082/metrics and require `X-Proxy-Key`. Put Prometheus on the internal
network and inject the header from a secret; do not publish the metrics ports.
Metrics cover request duration, outbox pending/oldest age/retry/delivered/lease reclaim,
and proxy failures. Counters are process-specific; backlog gauges reflect the database.

Back up before upgrading, run complete checks, then rebuild/start images. Migrations
run at startup. Never modify an existing migration; document compatibility and rollback
for new migrations. Check database compatibility before reverting images. API/worker
handle SIGTERM gracefully; unfinished jobs become reclaimable when their leases expire.

```powershell
docker compose --env-file .env.production --file infra/compose/compose.prod.yaml down
```

This stop command preserves data volumes. Upgrades do not require deleting volumes.

Validate HTTPS locally:

```powershell
./scripts/production-test.ps1 -Prepare
pnpm -C web test:deployment
./scripts/production-test.ps1 -Cleanup
```

The dedicated test entry `https://localhost:5443` uses Caddy's internal CA. Only test
clients ignore its certificate validation; no machine root certificate is installed.
`chat-deployment-test` has isolated volumes and subnet 172.30.241.0/24. Its configuration
is for local tests, not public deployment. Initial Linux image builds depend on network
and compiler resources.
