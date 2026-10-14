# Product Readiness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans after the user selects an execution method. Steps use checkbox syntax. This file coordinates the focused plans below.

**Goal:** Deliver the approved account, installation, notification and reaction work
with consistent documentation and revision-specific verification.

**Architecture:** Preserve the modular Rust application and static SvelteKit client.
Execute the focused plans in dependency order, with independent durable mail/push
loops and the existing transaction/replay owners. External release checks retain
their actual operator dependencies.

**Tech Stack:** Existing locked Rust 1.99/Axum/SQLx/PostgreSQL, SvelteKit 3/Svelte 5,
Vitest/Playwright, standard SMTP and Web Push; one root Compose infrastructure.

**Spec:** [Approved design package](../specs/2026-10-14-product-readiness-design.md).

**Local status (2026-10-15):** Implemented and verified by the final combined
`node scripts/check.mjs --integration` gate (exit 0). See the
[acceptance record](../../operations/product-readiness-acceptance.md) for source
identity, counts, review follow-up and external checks that remain pending.

## Global Constraints

- Engineering identifiers, comments, tests and documentation remain English; product UI remains Chinese.
- Preserve existing contracts and immutable migrations; regenerate Rust-owned API files.
- Keep one root `compose.yaml` and fixture-owned databases, buckets and processes.
- The user authorized the initial documentation commit `b472c6f`, then grouped local implementation/documentation commits on 2026-10-15. Pushes, PRs and deployment still require separate explicit instruction.
- iPhone/iPad real-device push acceptance is deferred and must remain explicitly unverified.
- Capacity/extended stability testing and production monitoring/backup automation remain outside this batch.
- Native background verification commands use `windowsHide` without Windows detached consoles.

## Review Focus

1. External checks may target a different build: Task 3 records and compares source/build SHA.
2. Interrupted verification can leak fixtures: every focused plan uses the owned cleanup recipe below.
3. Generated schemas can drift across subsystems: Task 2 runs generation/check on the combined revision.
4. Staging secrets can enter artifacts: Task 3 checks protected environment handling and existing redacted logging.
5. Deferred iOS tests can be mistaken for acceptance: Task 3 records a platform matrix with an explicit pending row.

## Focused plans and order

1. Task 1 below: operating-document and release baseline preparation.
2. [Account security](2026-10-14-account-security-implementation.md), tasks 1–6.
3. [PWA and push](2026-10-14-pwa-push-implementation.md), tasks 1–6.
4. [Message reactions](2026-10-14-message-reactions-implementation.md), tasks 1–3.
5. Tasks 2–3 below: integrated verification, documentation and external acceptance.

Execute migrations serially: the candidate names are `0012_account_security_versions.sql`,
`0013_account_email_and_mail.sql`, `0014_push_notifications.sql` and
`0015_message_reactions.sql`. At execution start, confirm the current maximum remains 11. If another approved change consumed a number, allocate the next free number and
record the adjustment; never renumber an already committed migration.

## Targeted verification commands

Run frontend unit targets from the repository root, for example:

```sh
pnpm -C web exec vitest run tests/unit/account-security.test.ts
pnpm -C web exec playwright test e2e/account-security.spec.ts
```

Plain Cargo tests cannot use the development database. This dependency-free recipe
acquires the existing isolated infrastructure and cleans up even on assertion failure.
Replace only the trailing Cargo arguments with the exact target named by a task:

```sh
node --input-type=module -e 'import { prepareTestInfrastructure, runCommand } from "./tests/infrastructure.mts"; const owned = await prepareTestInfrastructure(); try { await runCommand("cargo", ["test", "--all-features", ...process.argv.slice(1)], { env: { ...process.env, ...owned.values, CHAT_TEST_INFRASTRUCTURE_ENV: owned.envFile } }); } finally { await owned.cleanup(); }' -- --test account_security_test
```

The task shorthand **Rust target `account_security_test`** means that exact recipe
with `-- --test account_security_test`. Library adapter tests use `-- --lib mail`.
Run infrastructure-dependent targets serially. Do not print `owned.values` or read
development secrets. Format new test code before RED so a formatter failure is not
mistaken for a behavior failure.
The existing `test-failpoints` feature also gates the new fixture-only transports;
Task 3 of the account plan updates the root Cargo test phase to `--all-features`.
Production release builds retain default features and cannot enable those transports
through environment configuration.

### Task 1: current operations and release baseline

**Files:** Modify `docs/operations/groups-and-media.md`,
`docs/operations/continuous-integration.md`, `docs/operations/deployment.md` as needed.
Inspect `.github/workflows/ci.yml`, `tests/infrastructure.mts`, `infra/caddy/` and
`web/e2e/deployment.spec.ts`; do not change their behavior without a demonstrated issue.

**Interfaces:** Consumes the existing three scoped check commands and operator-owned
HTTPS deployment procedure. Produces an accurate baseline record identifying the
revision tested and external checks still pending.

- [x] Replace obsolete production Compose/setup-profile instructions in the groups/media guide with the current platform-independent deployment and `admin media-init` procedure.
- [x] Remove editing/search from that guide's future-work list and link current chat-experience documentation. Preserve historical acceptance records and their dates.
- [x] Check documentation links, commands and the English policy; run `node --test tests/architecture.test.mjs`. Expected: all architecture/documentation guards pass.
- [x] Review the existing CI fix and redacted artifact selection against current source. If a new failure is reproduced, add the actual regression before changing tooling; otherwise retain existing checks.
- [x] Record the exact local base and tested source-manifest identity. Hosted results for the unpublished implementation and staging actions remain pending until explicitly authorized/configured; older successes are not acceptance of this source.

This is a reversible documentation correction: do not add tests that assert wording
or reproduce implementation details solely to impose a TDD cycle.

### Task 2: integrate local behavior, contract and operations

**Files:** Modify `README.md`, `docs/architecture.md`,
`docs/operations/{development,deployment,continuous-integration,chat-experience}.md`;
create `docs/operations/account-security.md`, `docs/operations/pwa-notifications.md`
and `docs/operations/product-readiness-acceptance.md`.
Regenerate `web/openapi.json` and `web/src/lib/api-client/schema.d.ts`.

**Interfaces:** Consumes the focused plans' implemented database/API/cache contracts.
Produces the final build and a local acceptance record with actual command output.

- [x] Run `pnpm -C web api:generate`, then `pnpm -C web api:check`. Expected: generated contracts match Rust and all new schema references/operation IDs resolve.
- [x] Run `node scripts/check.mjs --integration`. Expected: format/lint, real Rust/media tests, worker typechecking, schema, units, static build, SDK/recovery and browsers all exit 0 with no test failure.
- [x] If a failure appears, diagnose its root cause, add a reproducing regression and fix it; rerun the affected scope. Repeat unrelated stages only when later edits invalidate their evidence.
- [x] Review all public routes, payloads, migrations, lock order, response fences, cancellation and secret-handling against the approved specs. Check that mail/push errors stay inside their independent loops.
- [x] Document SMTP/TLS/sender/public-origin/queue-key configuration, VAPID/provider allowlisting, enrollment/recovery/PWA updates and new event upgrade requirements. Defaults keep optional services disabled.
- [x] Record exact source revision or uncommitted diff identity, test counts/commands, migration range and external pending checks. Do not invent a final SHA before a requested commit exists.

### Task 3: hosted/staging and available-device acceptance

**Files:** Extend `web/e2e/deployment.spec.ts` with account, PWA and reaction checks;
modify deployment/acceptance documentation. Provider mailbox/device observations are
operator-owned inputs, not secrets committed to the repository.

**Interfaces:** Consumes `CHAT_DEPLOYMENT_ORIGIN`, separate media origin, fresh owned
invitations, protected SMTP/VAPID configuration and a disposable staging lifecycle.
Produces hosted job links and a per-platform verified/pending matrix for one build.

- [x] Extend staging regressions for real account password change, captured verification/reset mail where operator access permits it, manifest/worker assets and two-user reaction convergence.
- [ ] Use a fresh staging build and record its source SHA, build-time media origin and deployment identity before `pnpm -C web test:deployment`. Expected: all configured HTTPS scenarios pass with normal certificate validation.
- [ ] Verify a consenting staging mailbox receives the intended verification/reset link using required TLS; validate the full browser flow. Record actual arrival separately from SMTP acceptance.
- [ ] On an available desktop environment, enable push through a user gesture, close/background the page, send from another account and inspect the real OS notification/click behavior. Record browser/OS/provider and whether the browser remained running.
- [x] Mark iPhone/iPad real-device acceptance deferred by the user. Do not mark any other unavailable provider/device check passed; list it separately without blocking completed local work.
- [ ] After an explicitly requested push/PR, inspect Backend, Frontend and Contracts and integration for the final SHA. Expected: all three pass for the staging source revision, with safe artifacts and owned-resource cleanup.

## Execution record

The user approved native execution in the current branch on 2026-10-14 and declined
a worktree. Local implementation, one fresh whole-change review, its regression
follow-up and the integrated gate are complete. The execution base is `b472c6f`.
On 2026-10-15 the user authorized grouped local commits and removal of the completed
handoff file. The verified implementation is `cc76dc8`; the final documentation
commit records acceptance and separately pending operator/hosted checks.
No push, PR or deployment is authorized by those local commit instructions.
Ignored plan-owned ledgers under `.superpowers/sdd/` retain command evidence.
