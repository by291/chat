# Local development

Run commands from the repository root. Prerequisites are Rust 1.99.0, Node 24.21.0
or later, pnpm 12.11.2, and Docker with Compose v2. Windows and macOS can use Docker
Desktop; Linux can use Docker Engine. Shared tooling uses Node built-ins and does
not require PowerShell, Bash, or a task runner.

## Background development with Make

On macOS/Linux, the optional Makefile uses Process Compose to manage application
processes in the background. Install it once, separately from starting the app
(macOS: `brew install f1bonacc1/tap/process-compose`; other platforms: see the
[Process Compose installation guide](https://f1bonacc1.github.io/process-compose/installation/)).
`make dev` checks for the tool but never installs it. Frontend packages are installed
automatically with `pnpm install --frozen-lockfile` during background startup.

```sh
make dev-init      # Create local credentials; keep them if already initialized
make infra        # Start Docker dependencies and wait for readiness
make dev          # Return immediately; compile and start apps in the background
make dev-status    # Check build progress and API/web health
make dev-logs      # Follow logs; Ctrl+C leaves the apps running
make dev-stop      # Stop API, worker and web; Docker dependencies keep running
make infra-stop    # Stop Docker dependencies separately; keep their volumes
```

Wait for API and web to become healthy, then open `http://localhost:5173`.
Before detaching, `make dev` checks every Compose service and tells you to run
`make infra` if dependencies are missing or unhealthy. Startup builds the Rust
binaries, installs frontend packages, initializes the media bucket, and launches the
applications. A failed build/setup or an exited application stops the other managed
processes; inspect logs before retrying. Repeating `make dev` while the supervisor
is running prints a notice and exits successfully. Rust edits require `make dev-stop` followed
by `make dev`; Vite reloads frontend edits automatically.

The supervisor settings, socket and logs live in `.local/dev/` (an explicit
`PROC_COMP_CONFIG` overrides the settings directory). It only stops its own
application processes. Stop any manually started instances before `make dev`;
`make dev-stop` does not manage those terminals. Use `make infra-stop` after stopping
applications. Neither stop command removes database or media volumes. The commands
below remain available for foreground development without Make or Process Compose.

## First startup

```sh
pnpm -C web install --frozen-lockfile
node scripts/init-env.mjs
docker compose up -d --wait
cargo run --bin admin -- media-init
```

Initialize once. The initializer creates `.env` and `infra/seaweedfs/s3.json` with
random local secrets. If both files exist, it exits successfully without changing
them. If only one exists, it stops and asks you to restore the missing configuration;
existing credentials must continue to match the persisted services. Do not put
passwords on the command line or commit these files.

With Docker Engine on Linux, Centrifugo reaches the host via `host.docker.internal`
and the Docker bridge. In `.env`, set `INTERNAL_BIND_ADDR` to a host address reachable
from that bridge (for example `0.0.0.0:8081`), and allow only local/container peers
through the host firewall. Docker Desktop supports the default loopback listener.
This is a private authenticated callback endpoint, not public application ingress.

Start each application process in its own terminal:

```sh
cargo run --bin api
```

```sh
cargo run --bin worker
```

```sh
pnpm -C web dev
```

Open `http://localhost:5173`. Stop these foreground processes with Ctrl+C.
The public API and private proxy use ports 8080 and 8081; worker metrics use 8082.
PostgreSQL, Valkey, Centrifugo, and private SeaweedFS S3 use 55432, 56379, 8000, and 8333. Signed file transfers use the separate `http://localhost:8336` media ingress.
Vite proxies API/WebSocket requests through the browser's same origin. Development
uses an HttpOnly, SameSite=Lax `chat_session` cookie.

See [groups and media](groups-and-media.md) for endpoint, quota and upload limits,
and [UI components](../development/ui-components.md) for shadcn-svelte tooling.

## Accounts

Anyone can create an account at `/register`. Optionally create the first user
with the CLI; the terminal asks for the password twice without echo.
Passwords accept 6–128 Unicode scalar values. Operator prompts use ASCII English
for compatibility with legacy Windows console code pages.

```sh
cargo run --bin admin -- bootstrap --username alice
cargo run --bin admin -- reset-password --username alice
```

The display name defaults to the username; add `--display-name Alice` to override it.

Registration requires no invitation; a blank nickname uses the username.
Friend requests require acceptance before direct chat or friend-based group additions.
Group invitation links remain a separate joining mechanism. Password reset revokes all
sessions for the account. The device settings page can revoke an individual session.
The CLI requires database access; there is no separate web administration. Settings
support authenticated password changes. Optional verified email enrollment and
recovery require the [account mail configuration](account-security.md); mail and
[background push](pwa-notifications.md) remain disabled by default.

The friendship release changes the fresh-install identity migration. Pre-change
local databases must be recreated because their migration checksums differ; no
legacy conversation-to-friendship backfill is supported. Stop this project's
processes and recreate only its explicitly identified disposable development
database. Preserve source, environment files and unrelated Compose resources.
The test runner provisions its own fresh databases automatically.

## Checks and targeted tests

```sh
node scripts/check.mjs
node scripts/check.mjs --integration
node scripts/check.mjs --scope backend
node scripts/check.mjs --scope frontend
node scripts/check.mjs --scope integration
pnpm -C web api:generate
pnpm -C web api:check
```

Install the browser once before running integration checks:

```sh
pnpm -C web exec playwright install chromium
```

The base check includes architecture guards, Rust format/lint/tests (including real
storage cases), Svelte types, formatting, generated API verification, frontend units
and the static build. `--integration` also runs real SDK, recovery and browser tests.
The optional `--scope` runs one CI job locally. Frontend needs no Rust or Docker;
backend needs no frontend dependencies. Integration also verifies the generated API.
Rust DTOs own the contract; regenerate `web/openapi.json` and
`web/src/lib/api-client/schema.d.ts` rather than editing them manually. See
[the architecture map](../architecture.md) and root `AGENTS.md` for conventions.

Frontend unit tests do not need infrastructure. Targeted SDK/browser/recovery suites
own their infrastructure and clean up on completion:

```sh
pnpm -C web test
pnpm -C web test:delivery
pnpm -C web test:backup
pnpm -C web test:e2e -- e2e/auth.spec.ts
```

`compose.yaml` is the only infrastructure definition. Development uses `chat-dev`;
checks use `chat-test` with independent credentials, volumes and ports 55433, 56380,
8001, 8334, 8337, 18080, 18081, 18082 and 5174. Test configuration comes from
`.env.example`, without reading development secrets. Each fixture creates its own
database and bucket. Recovery tests use a fresh project and ports 55435, 56381, 8002,
8338, 8339, 19090, 19091 and 19092. These fixtures do not modify development data.

Run infrastructure-dependent suites serially. The check process owns middleware;
child suites borrow it through `CHAT_TEST_INFRASTRUCTURE_ENV`. A standalone suite
acquires the same infrastructure lock. Test helpers retain child-process objects
and stop only their own processes. Protected environment files, logs, copied native
binaries and recovery artifacts live in ignored `.local/tests/`; no PID files are
used. Full checks provide the isolated environment required by Rust integration
tests; a plain `cargo test` pointed at the development database is rejected.

On Unix, the check isolates each phase's process group and handles the first Ctrl+C
by letting that phase finish cleanup before exiting. Windows commands stay attached
to the existing console with additional windows suppressed. A terminal interrupt
can also reach a Windows test runner, so interrupted runs may require the cleanup
procedure below. Closing a console window is a forced interruption, not a graceful
fixture shutdown.

After a forcibly terminated run, first verify that no check or test is still running.
Stop any orphaned API/worker/frontend launched by that run, using the executable
paths under `.local/tests/run-*` to identify them. Then stop only test infrastructure:

```sh
docker compose --project-name chat-test --env-file .local/tests/infrastructure/test.env down
```

Remove `.local/tests/infrastructure.lock` only after those processes have stopped.
The lock contains no process identity; deleting it while a run is active would allow
conflicting tests. Normal completion releases it automatically. A recovery drill
uses a unique `chat-restore-drill-*` project; its matching `restore-*/stack.env` and
`compose.log` identify owned resources if interrupted. Remove volumes only for that
confirmed drill project. Retained logs aid failure diagnosis.

Stop development dependencies while preserving their data:

```sh
docker compose down
```
