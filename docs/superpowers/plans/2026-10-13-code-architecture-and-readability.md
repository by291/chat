# Code architecture and readability implementation plan

This completed plan is retained as historical context. Current tooling commands
are documented in [development](../../operations/development.md).

> **For agentic workers:** Use `superpowers:executing-plans` to implement this plan
> inline, with one fresh whole-project review at the end. Checkboxes track completion.

**Goal:** Make the complete Rust/frontend application readable and suitable for learning,
with English, enforceable conventions in `AGENTS.md`.

**Architecture:** Cohesive features with thin HTTP/UI adapters, explicit application
operations, concrete SQL repositories, owned DTOs and separate durable/ephemeral chat state.

**Tech stack:** Existing locked Rust/Axum/SQLx, Svelte/SvelteKit/TypeScript/TanStack Query,
Centrifugo, Valkey, PostgreSQL and SeaweedFS; no upgrades.

**Spec:** `docs/superpowers/specs/2026-10-13-code-architecture-and-readability-design.md`.

## Global constraints

- Current directory, root Rust package, `web/` frontend; no Git initialization or commits.
- English comments/documentation. Existing localized product copy and wire contracts remain.
- Existing schema/migrations, ordering, transaction boundaries, authorization and UI behavior remain.
- Preserve source snapshots/evidence; do not change live data or unrelated resources.
- Use blank lines for logical paragraphs and comments for reasons/invariants, not line narration.
- Add meaningful architecture checks; reuse the existing behavior suite for pure refactoring.

## Review focus

1. Concurrent registration/password reset/message retry must retain atomicity and lock order (Task 2).
2. Proxy refresh/revocation and worker retries must retain private identity and lease semantics (Task 3).
3. A delayed snapshot/event/send/read must not skip replay or acknowledge unrendered messages (Task 4).
4. Revalidation must retain drafts; IME, mobile return and older-history anchoring must stay usable (Task 5).
5. Documentation, generated API, CSS cascade and HTTPS build must describe and run the same application (Task 6).

## Task 1: Standards and architectural guardrails

- [x] Save baseline/snapshot and write `AGENTS.md`, the architecture map and this plan.
- [x] Add failing architecture tests for SQL ownership and frontend transport/dependency boundaries.
- [x] Configure readable Svelte formatting and integrate boundary checks into `check.ps1`.

## Task 2: Rust types, authentication and conversation workflows

- [x] Split shared IDs/sequence from feature DTOs; keep explicit `types` re-exports for existing consumers.
- [x] Move auth SQL to `auth/repository.rs`, rules to `validation.rs`, and keep transaction ownership in services.
- [x] Move conversation SQL to `conversations/repository.rs`; organize summaries, message/replay and receipts/typing workflows.
- [x] Expand SQL/macros, add meaningful paragraphs/docs, preserve lock/commit order, and run Rust/API checks.

## Task 3: Rust realtime, profiles and infrastructure

- [x] Separate proxy DTOs/service from HTTP, and queue persistence/delivery from worker orchestration.
- [x] Move profile/presence SQL into user repositories; remove business SQL from CLI/metrics adapters.
- [x] Clarify errors, config, bounded Lua/external calls, observability and startup/shutdown; review all Rust tests for readability.
- [x] Run Rust fmt/clippy/tests and the Rust architecture guards; compare API structure against the baseline.

## Task 4: Frontend transport and owned chat workflows

- [x] Consolidate typed transport/errors, add feature API adapters and explicit feature models/cache keys.
- [x] Extract the conversation store (snapshot/replay/generations) and peer activity (typing/presence/cleanup).
- [x] Keep the coordinator facade and all optimistic/rendered-read semantics; add focused extraction regressions only if needed.
- [x] Run types, unit tests, generated API checks and boundary guards.

## Task 5: Readable components and styles

- [x] Reformat all handwritten components/routes, name handlers/derived values, and explain lifecycle/visibility/IME invariants.
- [x] Remove HTTP construction from components; keep route composition thin and feature boundaries explicit.
- [x] Colocate feature styles and separate shared layout/theme styles while retaining cascade order.
- [x] Run formatting/types/build and real browser behavior/desktop-mobile checks.

## Task 6: English documentation and final acceptance

- [x] Translate README and existing public repository documentation into accurate English; update the ownership/reading map.
- [x] Run `check.ps1 -IncludeIntegration`, rebuilt HTTPS deployment and wire-contract comparison.
- [x] Dispatch one fresh reviewer; re-grade and fix Important/Critical findings once with regressions and the required suite.
- [x] Deliver English guidelines, representative reading paths, evidence and any material limitations; preserve current work.
