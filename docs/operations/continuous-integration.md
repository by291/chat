# Continuous integration

`.github/workflows/ci.yml` runs `node scripts/check.mjs --integration` on pushes,
pull requests targeting `main`, and manual dispatch. The runner is Ubuntu 24.04;
Rust, Node, pnpm and action references stay pinned by the repository. The workflow
installs Chromium and its Linux dependencies before running browser scenarios.

Local and CI checks share the root `compose.yaml` and the same Node entry point.
Test helpers start `chat-test` with separate credentials, ports and volumes derived
from `.env.example`; development `.env` and production secrets are not read. Rust
tests create unique databases and buckets. SDK/browser suites run native API and
worker processes with fixture-owned data; recovery tests restore into a fresh
`chat-restore-drill-*` project. Linux callback listeners are reachable from the
Docker bridge and authenticated with test-only internal credentials.

The command covers architecture guards, fmt/clippy, real Rust media/association
tests, Svelte types, formatting, generated API consistency, frontend units, the
static build, SDK integration, recovery and browser scenarios. Each fixture owns
its subprocesses; middleware stops in `finally`. The workflow repeats `chat-test`
shutdown after interruption. Dependency health checks bound startup; the job timeout
is 30 minutes. Logs and fixture configuration live in `.local/tests/`.

The base local command `node scripts/check.mjs` omits SDK/recovery/browser suites.
HTTPS checks remain separate and target an operator-provided staging deployment;
see [deployment](deployment.md). CI does not provision production infrastructure.

The token has `contents: read`; checkout does not persist Git credentials. The
workflow does not deploy or publish images. Writing a workflow is not an Actions
run: inspect its first hosted run after an explicitly requested push. Local results
do not establish the hosted runner's installation or network behavior.

Validate workflow syntax with `actionlint .github/workflows/ci.yml` when available.