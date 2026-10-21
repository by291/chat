# Continuous integration

`.github/workflows/ci.yml` runs on pushes, pull requests targeting `main`, and
manual dispatch. Three independent jobs start in parallel on Ubuntu 24.04. They
have no `needs` dependencies and do not share a workspace or test containers.

| Job                       | Shared local command                         | Responsibilities                                                                       | Required tools                     |
| ------------------------- | -------------------------------------------- | -------------------------------------------------------------------------------------- | ---------------------------------- |
| Backend                   | `node scripts/check.mjs --scope backend`     | Rust fmt/clippy and all Rust tests, including real media/associations                  | Node, Rust, Docker                 |
| Frontend                  | `node scripts/check.mjs --scope frontend`    | Architecture guards, Svelte/service-worker types, ESLint, formatting, units and static build | Node, pnpm                         |
| Contracts and integration | `node scripts/check.mjs --scope integration` | Generated API consistency, SDK authentication/delivery/presence, recovery and browsers | Node, pnpm, Rust, Docker, Chromium |

Contract generation belongs to the integration job because Rust owns the API and
the TypeScript generator needs frontend dependencies. Frontend checks use the
committed schema; the integration job rejects any drift. This lets frontend and
backend jobs start independently without installing each other's toolchains.

The shared check runner defaults `RUST_TEST_THREADS` to `2` so concurrent Rust
fixture migrations fit the isolated PostgreSQL server's shared-memory budget.
An explicit environment value overrides this default. This limits independent
test fixtures, not the concurrent operations exercised inside each test.
The root Compose file gives PostgreSQL a 256 MiB `/dev/shm` mount: the image can
already consume about 61 MiB at startup, leaving insufficient room for dynamic
query segments under Docker's 64 MiB default. Capacity and fixture concurrency
are explicit rather than hiding allocation failures with test retries.

Rust, Node, pnpm and Actions references remain pinned. pnpm caches its dependency
store; the backend and integration jobs cache Cargo dependencies/build artifacts
under separate job keys. Require all three job results when configuring branch
protection; a fast frontend result alone does not establish application correctness.
Update existing protection rules if they required the former combined job name.

## Test ownership

Optional account mail and background-push scenarios use owned capture providers;
Rust and Node receivers verify actual TLS/signatures/encryption. The push browser
scenario uses full Chromium's new headless mode rather than the limited headless
shell. The existing `playwright install --with-deps chromium` step supplies it.
Local Windows runs may set `CHAT_TEST_PUSH_BROWSER_CHANNEL=chrome` or `msedge` when
the pinned full Chromium cannot run; that uses a fresh test profile and does not
replace hosted acceptance of CI's pinned browser. Production builds omit
`test-failpoints`, keeping fixture destination/certificate overrides unavailable.

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

SeaweedFS 4.48 drops from root to its `seaweed` user before reading S3 credentials.
Fixture credential files keep mode `0600` and their host user's ownership. When
that UID differs from Seaweed's UID 1000, a direct bind mount is unreadable after
the privilege drop. The root Compose wrapper copies the read-only source into a
container-private directory (mode `0700`) with a
`seaweed`-owned file (mode `0600`), then executes the image's original entrypoint.
This preserves upstream `/data` ownership repair, privilege dropping and FIPS
configuration without broadening host-file permissions.

On command failure, the launcher includes at most the final 8 KiB of the current
command's log, redacting known fixture credentials and discarding a truncated
first line. Failed Compose startup also collects `ps --all` and the last 100 log
lines per owned service before cleanup, with a 15-second deadline per diagnostic
command. Each output section is bounded to 8 KiB. A diagnostic or cleanup failure
is retained alongside the original startup error rather than replacing it.
Raw command logs and diagnostic reports are created with mode `0600` on Unix.
Timeout rejection does not wait for inherited pipes or log reading. Unix cancels
the owned process group; Windows attempts hidden `taskkill /T /F` for a still-running
owned child, with a bounded fallback to its native process handle. Already-exited
Windows PIDs are skipped to avoid targeting a reused ID.

The same redacted report is printed and saved as `compose-diagnostics.log` beside
the fixture environment file. Backend/integration failure artifacts allow only
`.local/tests/**/compose-diagnostics.log` and expire after seven days. Hidden-file
upload is enabled for that explicit filename allowlist; raw `compose.log`, `test.env`
and S3 JSON credential files are never included. Keep those raw local files private.

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
The Linux permissions failure was reproduced on an isolated native Linux Docker
volume. With the private-copy wrapper, all five middleware services became healthy;
the source remained root-owned `0600`, the private copy was owned by UID 1000, and
Seaweed ran under UID 1000 with the upstream FIPS setting. A deliberately failed
Compose startup also verified the real redacted diagnostic and cleanup path.
These local checks do not establish a hosted result for unpushed changes.
Dependabot configuration takes effect on the default branch; security-alert and
security-update repository settings are separate from scheduled version updates.

Validate the workflow with `actionlint .github/workflows/ci.yml` and validate
`dependabot.yml` against its JSON Schema when changing their structure.
