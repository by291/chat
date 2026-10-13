# Local development

Run commands from the repository root. Prerequisites are Rust 1.99.0, Node 24.21.0
or later, pnpm 12.11.2, Docker Desktop, and PowerShell 7.

```powershell
# Initialize once; skip this command when .env already exists.
./scripts/dev.ps1 -InitEnv
./scripts/dev.ps1 -Up
pnpm -C web install --frozen-lockfile
```

Start the API, worker, and frontend in the background. `-Stop` stops only the
processes recorded by this launcher; it preserves the database and containers.

```powershell
./scripts/dev.ps1 -Start
./scripts/dev.ps1 -Stop
```

Alternatively, observe each process in its own terminal:

```powershell
cargo run --bin api
cargo run --bin worker
pnpm -C web dev
```

Open `http://localhost:5173`. The public API and private proxy bind to loopback ports
8080 and 8081; worker metrics use 8082. Development uses the HttpOnly, SameSite=Lax
`chat_session` cookie. PostgreSQL, Valkey, Centrifugo, and SeaweedFS host ports are
55432, 56379, 8000, and 8333. Vite proxies API/WebSocket requests through the browser's
same origin. Background process records and logs live in `.superpowers/dev/`.

Create the first user. The terminal asks for the password twice without echo:

```powershell
cargo run --bin admin -- bootstrap --username alice --display-name 'Alice'
```

Generate an invitation for another user. Enter its token on the registration page,
or use `/register?invite=<token>`. An invitation is single-use and expires after seven
days. Password reset revokes all sessions for the account.

```powershell
cargo run --bin admin -- invite
cargo run --bin admin -- reset-password --username alice
```

The device settings page can revoke an individual session. The CLI relies on
database access; phase one has no separate web administration or email recovery.

```powershell
./scripts/check.ps1
./scripts/check.ps1 -IncludeIntegration
pnpm -C web api:generate
pnpm -C web api:check
```

Rust DTOs own the contract. Regenerate `web/openapi.json` and
`web/src/lib/api-client/schema.d.ts`; never edit them by hand. `check.ps1` includes
architecture ownership guards, Rust formatting/lint/tests, Svelte types, frontend
formatting/unit tests, contract verification, and the static build. See
[the reading guide](../architecture.md) and root `AGENTS.md` for conventions.

`web/pnpm-workspace.yaml` contains pnpm 12 dependency-script policy. This application
still consists of one Rust package and one frontend package.

Integration checks use the isolated `chat-e2e` Compose project and databases, with
ports 55433, 56380, 8001, 8334, 18080, 18081, and 5174. They do not modify `chat-dev`
data. Run the suite serially. After an interrupted run, execute
`./scripts/e2e.ps1 -Cleanup`. Original phase-one evidence lives in
`.superpowers/sdd/2026-10-12-phase-1-private-chat/`; architecture-refactor evidence and
the original-source snapshot live in
`.superpowers/sdd/2026-10-13-code-architecture-and-readability/`. Scratch is excluded
from version control and must be retained when no Git history exists.

Stop development infrastructure while preserving data volumes:

```powershell
docker compose --project-name chat-dev --env-file .env --file infra/compose/compose.dev.yaml down
```
