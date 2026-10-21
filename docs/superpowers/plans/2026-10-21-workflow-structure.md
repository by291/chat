# Workflow Structure and Reliability Refactor

**Goal:** Make the Rust and Svelte code navigable by business workflow, clarify state and transaction ownership, and fix the audited reliability issues without changing product behavior.

**Architecture:** Keep Rust at the repository root and SvelteKit in `web/`. Group cohesive workflows beneath existing features; retain explicit compatibility exports only where existing Rust consumers need them. The authenticated workspace owns shared lifetime, while each feature owns its records, requests and presentation.

**Tech stack:** Rust, Axum, SQLx/PostgreSQL, Svelte 5, SvelteKit, TanStack Query, TypeScript.

**Spec:** The approved review and user request: reorganize both stacks, improve responsibilities/readability/comments, fix previously identified issues, preserve functionality, and do not commit.

## Constraints

- Preserve HTTP routes, generated contracts, JSON payloads, errors, SQL semantics and UI interactions.
- Preserve lock order, lease fencing, at-least-once delivery, contiguous replay, DOM-confirmed read cursors and same-user session lifetime.
- No ORM, generic service framework, duplicate server records, or new product features.
- Run direct frontend tools from `web/`; do not create root frontend artifacts.
- Keep all changes uncommitted in the existing checkout. Independent domains may execute concurrently with disjoint ownership; cross-domain import rewrites are integrated afterward.
- Existing behavior tests cover structural moves; add regressions for confirmed bugs and meaningful ownership invariants.

## Tasks

### 1. Backend workflow structure and reliability

- [x] Group conversation messages, groups/membership/invitations and preferences with their persistence/HTTP boundaries; organize auth security/mail and media workflows where currently flat.
- [x] Split group invitation operations from membership policy without duplicating aggregate locks or role policy.
- [x] Bound cancellation through outbox claim/delivery/completion and report durable completion failures separately from lost leases.
- [x] Move push binding expiry transaction orchestration out of its repository into its application workflow.
- [x] Centralize persisted outbox payload contracts while retaining serialized compatibility.
- [x] Improve logical phases, descriptive names and lock/transaction contract comments in touched workflows, including friendships.
- [x] Preserve public Rust compatibility paths and add focused reliability/contract regressions.

### 2. Chat state and workflow structure

- [x] Organize chat conversations, messages, groups, synchronization and alerts into cohesive directories.
- [x] Normalize shared summary/replay promises so concurrent callers observe the same handled failure; demonstrate the prior failure with regression tests.
- [x] Make mutable stream state private; expose explicit reload/read-acknowledgement operations and read-only observations.
- [x] Restrict chat teardown to chat-owned cache entries; the workspace owns whole-session cache teardown.
- [x] Improve logical phases, request-generation comments and component readability while preserving UI behavior.

### 3. Workspace, contacts and settings ownership

- [x] Move application navigation and authenticated lifetime composition to a workspace feature.
- [x] Keep route files focused on composition and navigation; preserve push stop/dispose and same-user revalidation behavior.
- [x] Separate contacts page orchestration from list/detail presentation, keeping URL selection canonical and search pagination cancellable.
- [x] Organize friends/requests and settings profile/security/appearance by workflow.
- [x] Restore logical whitespace and meaningful lifecycle comments in affected components/state owners; preserve localization and accessibility.

### 4. Integration, documentation and guardrails

- [x] Reconcile imports and actual directory ownership; update architecture reading paths and style ownership.
- [x] Extend architecture checks for nested workflow APIs and transaction/state boundaries without permissive arbitrary API locations.
- [x] Distinguish formatting from semantic linting and add appropriate stack-aware enforcement where feasible.
- [x] Review all diffs, including new files, for behavior changes and responsibility drift.
- [x] Run `node scripts/check.mjs --integration`, including existing browser behavior coverage, and resolve failures.
- [x] Verify generated contracts unchanged, no root frontend artifacts, and no commit or push.

## Review Focus

- Concurrent summary/replay failures, membership removal and retries must not leak unhandled rejections or stale responses.
- Cancellation during fanout/completion must leave a safely reclaimable lease and never falsely acknowledge delivery.
- Logout clears every session-owned cache; ordinary chat disposal must not destroy unrelated feature data.
- Profile refresh, URL back/forward, combined requests, mobile list/detail and read acknowledgement behavior remain unchanged.
- Directory moves must preserve CSS order, runtime imports, public Rust paths and generated OpenAPI output.

## Execution Record

- Baseline: clean `main` at `fb6b711`; no pending source changes. Prior UI verification is recorded separately.
- Parallel ownership: backend agent owns Rust source/tests; chat agent owns chat source and chat unit regressions; workspace agent owns workspace/contacts/settings/routes. Coordinator owns cross-domain integration, architecture guards and documentation.

- Work branch: `feature/workflow-structure`; changes remain unstaged/uncommitted.
- Frontend review: no actionable regressions after comparing moved files, session/replay fences, contacts behavior and responsive CSS.
- Backend review: preserve the legacy push-expiry entry point and roll back binding changes when final lease CAS misses. Both fixes passed scoped re-review and focused runtime verification.
- First full gate completed 200 Rust tests successfully; intentionally cancelled between phases to incorporate review fixes, with owned infrastructure cleaned up. This is not a completed integration gate.
- Shared-flight and chat-cache regressions failed before their fixes and now pass; targeted frontend chat suites passed 73 tests and workspace/account/contact suites passed 31 tests.
- Both worker blocked-claim cancellation and blocked autocommit completion defects were reproduced against PostgreSQL before their fixes; targeted backend suites passed 68 tests before final review fixes.
- Review fixes: the original push-expiry signature is restored through a pure compatibility facade; a zero-row completion CAS explicitly rolls back binding effects. The lock-wait regression failed before the fix and passed afterward.
- Targeted push/subscription/worker verification passed 28 tests with two independent fixtures at a time. The earlier unbounded run exhausted PostgreSQL shared memory during fixture migrations; the check runner now defaults to two Rust test threads while retaining explicit environment overrides and concurrency inside each test.
- A subsequent full run still hit PostgreSQL shared-memory exhaustion with two fixtures. Direct inspection found 61 MiB of the default 64 MiB mount used at startup; the root Compose definition now allocates 256 MiB. The active test container confirms 256 MiB capacity and headroom under load.
- With corrected infrastructure, `node scripts/check.mjs --integration` passed 32 architecture checks, 201 Rust tests, 150 frontend unit tests, 12 SDK/recovery integration tests, Rust/Svelte/TypeScript lint and type checks, formatting, generated API verification and the static build. Svelte reported zero errors and warnings.
- The browser phase passed 52 of 53 scenarios. The remaining scenario completed group creation but its global heading locator matched both the existing chat heading and overview title. Scoped the assertion to `.chat-heading`, consistent with the group-management test; no product code changed. Both user-picker scenarios then passed in an isolated rerun, completing coverage of all 53 scenarios. The original full command exited unsuccessfully; this result combines its passing phases with the focused rerun, rather than claiming a single green full invocation.
- Final source checks found no whitespace errors, no generated contract changes, no root `node_modules`/`.svelte-kit`, and no staged changes or new commits. Test infrastructure was cleaned up. Work remains uncommitted on `feature/workflow-structure` at `fb6b711`.
