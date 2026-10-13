# Continuous integration

`.github/workflows/ci.yml` runs `scripts/check.ps1` on pushes, pull requests targeting
`main`, and manual dispatch. The runner is Ubuntu 24.04; Rust, Node, pnpm and action
references stay pinned by the repository.

Local and CI checks use the root `compose.yaml` as the only middleware definition.
`scripts/e2e.ps1 -PrepareInfrastructure` starts the independent `chat-test` project
with test-only credentials, separate ports and volumes. Its configuration is derived
from `.env.example`; development `.env` and production secrets are not read. The
check entry point loads that environment for Rust tests, which create randomly
named databases and buckets inside the test instance.

The shared command covers architecture guards, fmt/clippy, all Rust cases including
real media/association tests, Svelte types, formatting, generated API consistency,
frontend units and the static build. It shuts down only `chat-test` in `finally`.
The workflow repeats that cleanup after interruption. Compose service health checks
bound dependency startup; the job timeout is 30 minutes.

The base CI does not run the Windows native SDK/browser/recovery launcher.
`./scripts/check.ps1 -IncludeIntegration` adds those suites locally using the same
infrastructure. HTTPS checks target an already deployed staging application as
specified in [deployment](deployment.md), rather than a separate Compose stack.

The token has `contents: read`; checkout does not persist Git credentials. The
workflow does not deploy or publish images. Writing a workflow is not an Actions
run: inspect its first hosted run after an explicitly requested push. Local check
results do not establish the hosted runner's installation or network behavior.

Validate workflow syntax with `actionlint .github/workflows/ci.yml` when available.
