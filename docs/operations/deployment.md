# Production deployment

Deploy the Rust API and worker plus the built frontend. Choose a process manager,
containers or hosted services according to the host; production does not require
Compose. The root `compose.yaml` is for development and isolated tests.

## Services and connections

| Component | Responsibility | Required connections |
| --- | --- | --- |
| `api` | REST, sessions and migrations | PostgreSQL, Valkey, SeaweedFS |
| `worker` | Durable realtime delivery and media jobs | PostgreSQL, Valkey, Centrifugo, SeaweedFS |
| `web/build/` | Static SvelteKit SPA | Served by the reverse proxy; no resident Node server |
| PostgreSQL | Durable users, messages, events and attachment state | Private application access |
| Valkey | Temporary rates, presence and realtime history | Private API, worker and Centrifugo access |
| Centrifugo | WebSocket notifications | Valkey and the API's private connect/refresh callbacks |
| SeaweedFS | Private S3 objects | API/worker and signed transfers through the media proxy |
| Reverse proxy | HTTPS, static files, REST, WebSocket and media ingress | API, Centrifugo and SeaweedFS |

For one host, application/private service listeners can use loopback. Only the
reverse proxy needs public HTTP/HTTPS access. With separate hosts, use private
service addresses and restrict access to the relevant peers. Keep media on a
separate origin so uploaded original files do not share the application's cookies.

## Build and application configuration

Build on the target platform, using the repository's locked Rust toolchain and pnpm
version. Set the final media origin before compiling the frontend because it is
included in the browser's content security policy.

```powershell
cargo build --locked --release --bin api --bin worker --bin admin
pnpm -C web install --frozen-lockfile
$env:CHAT_MEDIA_ORIGIN='https://media.example.com'
pnpm -C web build
```

Install the three binaries and `web/build/` on the host. Configure API, worker and
admin with the same protected environment; the Rust binaries also read `.env` in
their working directory. Use independently generated credentials whose actual
values match the PostgreSQL, Centrifugo and SeaweedFS services.

```dotenv
DATABASE_URL=postgres://chat:<database-password>@127.0.0.1:5432/chat
VALKEY_URL=redis://127.0.0.1:6379
ALLOWED_ORIGINS=https://chat.example.com
COOKIE_SECURE=true
BIND_ADDR=127.0.0.1:8080
INTERNAL_BIND_ADDR=127.0.0.1:8081
WORKER_METRICS_ADDR=127.0.0.1:8082
TRUSTED_PROXY_IPS=127.0.0.1
CENTRIFUGO_API_URL=http://127.0.0.1:8000/api
CENTRIFUGO_API_KEY=<private-api-key>
INTERNAL_PROXY_KEY=<private-callback-key>
MEDIA_S3_INTERNAL_ENDPOINT=http://127.0.0.1:8333
MEDIA_S3_PUBLIC_ENDPOINT=https://media.example.com
MEDIA_S3_BUCKET=chat-media
MEDIA_S3_REGION=us-east-1
SEAWEED_ACCESS_KEY=<s3-access-key>
SEAWEED_SECRET_KEY=<s3-secret-key>
MEDIA_USER_QUOTA_BYTES=1073741824
GROUP_MAX_MEMBERS=100
RUST_LOG=chat_server=info
```

`TRUSTED_PROXY_IPS` must contain only the actual reverse proxy peer addresses.
Configure the proxy to replace `X-Forwarded-For` with the client address. Changing
`MEDIA_S3_PUBLIC_ENDPOINT` also requires rebuilding with the matching
`CHAT_MEDIA_ORIGIN`. API startup applies embedded migrations; start it before the
worker. Use graceful process shutdown, allow up to 90 seconds for draining, and
configure restart on failure in the chosen process manager.

Run the installed `admin` with the same environment to initialize storage and create
the first account. Passwords are entered interactively, never as arguments.

```sh
./admin media-init
./admin bootstrap --username alice --display-name Alice
./admin invite
```

## Realtime, storage and proxy

Use `infra/centrifugo/config.json` as a reference. Replace its development Valkey
address and `host.docker.internal` callbacks with the actual private addresses:
`http://127.0.0.1:8081/internal/centrifugo/connect` and `/refresh` on a single host.
Set the allowed origin to the application origin. Inject the private API key and
both static `X-Proxy-Key` callback headers through Centrifugo's environment. The
keys must match the application's `CENTRIFUGO_API_KEY` and `INTERNAL_PROXY_KEY`.
The supplied channel namespace and recovery/presence configuration remain needed.

Configure SeaweedFS S3 identities with the application's access/secret keys, persist
its master/filer/volume data, and keep its S3 listener private. The development
initializer's `infra/seaweedfs/s3.json` demonstrates the identity format; production
credentials and data have their own lifecycle. See [groups and media](groups-and-media.md)
for signed transfer and quota behavior.

Caddy is a supplied reverse proxy example. Copy `infra/caddy/Caddyfile`,
`media.caddy` and `logging.caddy` into `/etc/caddy/`, and pass these environment
variables to the Caddy process:

```dotenv
CHAT_DOMAIN=chat.example.com
CHAT_ORIGIN=https://chat.example.com
CHAT_MEDIA_ORIGIN=https://media.example.com
CHAT_WEB_ROOT=/opt/chat/web
CHAT_API_UPSTREAM=127.0.0.1:8080
CHAT_CENTRIFUGO_UPSTREAM=127.0.0.1:8000
CHAT_MEDIA_UPSTREAM=127.0.0.1:8333
MEDIA_S3_BUCKET=chat-media
```

Resolve both public domains to the host. `CHAT_WEB_ROOT` points to the deployed
contents of `web/build/`. Caddy obtains certificates and needs persistent, writable
certificate storage. Its configuration serves deep links through `index.html`,
proxies `/api/v1/*` and `/health/*`, upgrades `/connection/websocket`, and rejects
`/internal/*`, `/metrics` and Centrifugo `/api/*` publicly.

The media example permits only signed staging PUT and immutable GET/HEAD with exact
application-origin CORS. Preserve Host and path for S3 signatures and strip cookies
before forwarding to S3. Keep its body/time limits, `nosniff` and private/no-store
headers when using another proxy. Logging excludes cookies, authorization, query
strings and invitation tokens. API error responses and logs share `X-Request-ID`.
Private API and worker metrics require `X-Proxy-Key`.

## Verify a deployed staging application

On the selected deployment platform, first check readiness, SPA deep links, Secure /
HttpOnly session cookies, WebSocket connection and signed file upload/download.
The existing browser checks can target that staging application without database
access or provisioning a Docker stack:

```powershell
$env:CHAT_DEPLOYMENT_ORIGIN='https://staging.example.com'
$env:CHAT_DEPLOYMENT_MEDIA_ORIGIN='https://media-staging.example.com'
$env:CHAT_DEPLOYMENT_INVITE_1='<first-fresh-staging-invitation>'
$env:CHAT_DEPLOYMENT_INVITE_2='<second-fresh-staging-invitation>'
./scripts/production-test.ps1 -Prepare
pnpm -C web test:deployment
```

Generate the two invitations with the deployed staging `admin invite`; each run
consumes them and creates two fixture accounts, a conversation and media objects.
Use a disposable staging database/bucket, and clean it through that environment's
normal lifecycle. `CHAT_DEPLOYMENT_BUCKET` defaults to `chat-media`; set it when the
staging bucket differs. Certificate validation stays enabled. Only a local staging
CA may set `CHAT_DEPLOYMENT_INSECURE_TLS=true`; no machine trust store is modified.
No staging address is assumed, and these checks are separate from local test suites.

## Upgrade and recovery

Back up PostgreSQL and the corresponding SeaweedFS data together before migrations
or storage upgrades. Close public mutation/media PUT ingress, drain API and worker,
then take the database dump and a stopped storage snapshot. Restart dependencies,
API and worker before reopening ingress. Keep the matching protected configuration
with each backup, and verify recovery into an isolated environment before relying
on it. See [backup and restore](backup-restore.md) for the local development utility.
A native/hosted deployment uses its provider or host tools for the same procedure.

Migrations run on API startup. Keep existing migrations immutable and check schema
compatibility before rolling back binaries. Persist service data across upgrades;
rebuilding the application never requires deleting PostgreSQL or SeaweedFS data.
