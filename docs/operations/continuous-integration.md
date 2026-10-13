# Continuous integration

`.github/workflows/ci.yml` runs on pushes, pull requests targeting `main`, and
manual dispatch. Three independent jobs start in parallel on Ubuntu 24.04. They
have no `needs` dependencies and do not share a workspace or test containers.

| Job | Shared local command | Responsibilities | Required tools |
| --- | --- | --- | --- |
| Backend | `node scripts/check.mjs --scope backend` | Rust fmt/clippy and all Rust tests, including real media/associations | Node, Rust, Docker |
| Frontend | `node scripts/check.mjs --scope frontend` | Architecture guards, Svelte types, formatting, units and static build | Node, pnpm |
| Contracts and integration | `node scripts/check.mjs --scope integration` | Generated API consistency, SDK authentication/delivery/presence, recovery and browsers | Node, pnpm, Rust, Docker, Chromium |

Contract generation belongs to the integration job because Rust owns the API and
the TypeScript generator needs frontend dependencies. Frontend checks use the
committed schema; the integration job rejects any drift. This lets frontend and
backend jobs start independently without installing each other's toolchains.

Rust, Node, pnpm and Actions references remain pinned. pnpm caches its dependency
store; the backend and integration jobs cache Cargo dependencies/build artifacts
under separate job keys. Require all three job results when configuring branch
protection; a fast frontend result alone does not establish application correctness.
Update existing protection rules if they required the former combined job name.

## Test ownership

Local and CI checks share the root `compose.yaml`. Each infrastructure-dependent
runner starts its own `chat-test` project with separate credentials, ports and
volumes derived from `.env.example`. Development `.env` and production secrets are
not read. The frontend job starts no infrastructure.

`tests/infrastructure.mts` owns middleware, environment and argv execution using
only Node built-ins, so backend checks do not require frontend dependencies.
Rust tests create unique databases and buckets. SDK/browser suites run fixture-owned
native API/worker processes; recovery tests use a fresh `chat-restore-drill-*`
project. Each suite stays serial inside its own runner to preserve fixed-port and
native-binary ownership. Parallelism is between isolated CI runners.

Middleware stops in `finally`, and backend/integration jobs repeat `chat-test`
shutdown after interruption. Logs and protected fixture configuration live in
`.local/tests/`. Dependency health checks bound startup. Backend/integration have
30-minute job deadlines; frontend has 15 minutes.

The existing local `node scripts/check.mjs` and
`node scripts/check.mjs --integration` commands retain their base/full behavior.
Run infrastructure-dependent scopes serially on one developer machine.
HTTPS checks still target an operator-provided staging deployment; see
[deployment](deployment.md).

## Dependency updates

`.github/dependabot.yml` checks Cargo at `/`, pnpm through the `npm` ecosystem at
`/web`, GitHub Actions at `/`, and root Compose images through `docker-compose`.
It checks weekly on Mondays at 09:00 Asia/Shanghai, groups minor/patch updates by
ecosystem, leaves major updates as individual PRs, and allows up to three open
version-update PRs per ecosystem. Updates require review and the normal CI checks.
Automatic merging is not enabled.

GitHub's [support matrix](https://docs.github.com/en/code-security/reference/supply-chain-security/supported-ecosystems-and-repositories)
currently lists pnpm 7–10. The
[upstream updater](https://github.com/dependabot/dependabot-core/blob/main/npm_and_yarn/lib/dependabot/npm_and_yarn/helpers.rb)
already handles pinned pnpm 12 and version-9 lockfiles, which this repository uses.
The hosted rollout can lag the upstream source; verify the first frontend update
job before claiming pnpm 12 updater acceptance. The project's package-manager
version is preserved.

## Verification limits

The workflow token has `contents: read`; checkout does not persist credentials.
CI does not deploy or publish images. Writing configuration is not a hosted run:
inspect the first Actions and Dependabot runs after an explicitly requested push.
Dependabot configuration takes effect on the default branch; security-alert and
security-update repository settings are separate from scheduled version updates.

Validate the workflow with `actionlint .github/workflows/ci.yml` and validate
`dependabot.yml` against its JSON Schema when changing their structure.
