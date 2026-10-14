# Chat Experience Implementation Plan

> **For agentic workers:** Use superpowers:subagent-driven-development for the
> independent implementation tasks and review the complete integration afterwards.

**Goal:** Finish the requested CI repair and everyday chat features.

**Architecture:** Extend the existing conversation transactions, durable event
stream and query cache. Keep ephemeral notification state separate from message
replay, and persist per-user preferences in PostgreSQL.

**Tech Stack:** Existing Rust/Axum/SQLx, PostgreSQL, Svelte/shadcn, Centrifugo and Node.

**Spec:** ../specs/2026-10-14-chat-experience.md

## Constraints and review focus

- English code/docs; Chinese UI; one root compose.yaml; no automatic commit/push.
- Coordinate Cargo/native infrastructure runs to avoid executable locks and excess
  memory use. Hide Windows subprocesses; clean up only fixture-owned resources.
- Preserve send idempotency after mutations, reject foreign/old-episode targets,
  prevent stale responses from resurrecting tombstones, and never mark contextual
  history read through an unseen latest cursor.
- Privacy review includes persisted event payloads, reply previews, media grants,
  personalized preference notifications and system notification contents.
- Notifications are open-page only, as explicitly selected by the user.

## Task 1: CI repair and diagnostics

Files: .github/workflows/ci.yml, tests/infrastructure.mts, compose.yaml when the
reproduced cause requires it, docs/operations/continuous-integration.md.

- [x] Inspect the failing run and reproduce middleware startup in a clean Linux setting.
- [x] Add a meaningful failure-output regression before repairing the observed cause.
- [x] Keep Compose diagnostics visible/bounded and preserve failure/cleanup errors.
- [x] Validate workflow structure and real middleware startup; document hosted limits.

## Task 2: Rust message, search and preference workflows

Files: migrations/, src/conversations/{model,messages,repository,http,service}.rs,
cohesive mutation/search/preference modules when needed, src/app.rs, src/types/,
src/media/ authorization boundaries, tests/ conversation integration regressions.

- [x] Agree and publish exact DTO/endpoint/event names to the frontend implementer.
- [x] Write real database regressions for replies, author-only mutation, send retry
  after edits, tombstone replay/media authorization, Chinese search and membership fences.
- [x] Add immutable migrations, atomically append mutations and personalized preference
  invalidations, implement scoped search/context and regenerate the API schema.
- [x] Verify revision-aware persistence, summary enrichment and existing contracts.

Task 2 verification: `cargo check --all-targets`,
`cargo clippy --all-targets --all-features -- -D warnings`,
`node scripts/generate-api.mjs --check` and architecture guards passed. Real
PostgreSQL/Valkey/SeaweedFS checks passed all 8 experience tests and all 8 media
association tests, including ignored media cases. The shared fixture owner cleaned
up `chat-test` after the run. Full integration remains Task 4.
The populated-schema upgrade regression also passed: genuine SQLx migrations
through `0010`, a legacy Unicode message with ordered attachment references,
then `0011`, original-input retries, default preferences and post-upgrade mutations.
The full run exposed an existing concurrency test that identified its pause point
by one-line SQL text. Its gate now observes the actual ungranted PostgreSQL
`messages` relation lock; the original membership snapshot assertions pass.

## Task 3: Frontend workflows and notifications

Files: web/src/lib/features/chat/, related settings components, web/tests/unit/,
web/e2e/ experience scenarios. Consume generated DTOs from Task 2.

- [x] Cover revision-aware cache merges and notification suppression with focused tests.
- [x] Extend the feature API/state owners with reply/edit/withdraw, seek search/context,
  preferences and archive views; keep components free of transport/consistency rules.
- [x] Reuse shadcn controls and preserve IME, draft lifetime, mobile layout and scrolling.
- [x] Add open-page notifications, user-controlled sound/system permissions and cleanup.
- [x] Verify typing, unit tests and real multi-device browser scenarios.

## Task 4: Integration, review and delivery

- [x] Review cross-task schema, events, race handling, media/privacy and ownership.
- [x] Run node scripts/check.mjs --integration once changes are integrated; use scoped
  checks for newly discovered failures rather than repeating unrelated stages.
- [x] Run an independent whole-change review and fix material findings.
- [x] Update architecture, operations, README and acceptance evidence to final behavior.
- [x] Report actual results and hosted/staging limits; leave commits/pushes to the user.

## Final verification: 2026-10-14

- The root full integration entry point exercised Rust format, strict Clippy,
  90 Rust tests including real storage and the populated migration, generated API
  consistency, frontend checks, and all 7 real SDK/recovery tests.
- Its browser stage exposed a notification ordering race: receipt-triggered replay
  could apply a message before its exact live hint. Regressions now cover that
  ordering, acknowledged reads while summary refresh is pending, and duplicate hints
  after focus/mute suppression. No notification path advances a read/event cursor.
- After the fixes, `node scripts/check.mjs --scope frontend` passed 26 architecture
  guards, 78 unit tests, Svelte types with zero errors/warnings, formatting and the
  static build. All 27 Chromium browser scenarios passed in 4.8 minutes under one
  shared infrastructure owner, using the same environment borrowing as the root
  check. Unchanged Rust/SDK phases were not repeated for frontend-only fixes.
- Real browser cases cover reply/edit/withdrawal convergence while offline, Chinese
  seek pagination, unloaded contextual navigation without unintended reads, personal
  preferences across devices, settings-page alert navigation and teardown, and first
  direct/group messages while metadata is delayed. Permission/audio browser APIs
  are controlled in the notification test; REST, worker and Centrifugo are real.
- Desktop reply and 390-pixel mobile search screenshots were inspected. Independent
  backend, CI and frontend integration reviews have no unresolved findings.
- Native Linux middleware startup and the actual redacted failure-diagnostics path
  passed separately, including private SeaweedFS credentials and UID 1000 execution.
  Hosted Actions cannot be confirmed until these changes are pushed. No staging
  deployment origin was configured, so HTTPS deployment checks were not run.
- Architecture, operations, README and this plan describe the final behavior.
  Local delivery is grouped into CI, complete chat functionality and documentation
  commits. No push or deployment was performed.
