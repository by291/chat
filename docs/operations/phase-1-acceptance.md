# Phase-one acceptance

Scope: working private text chat, a root Rust package and `web/` frontend, with Chinese
desktop/mobile UI. Validation uses actual PostgreSQL, Valkey, Centrifugo, SeaweedFS,
and Chromium rather than static conversations or a mocked backend.

| Capability | Evidence |
| --- | --- |
| Single-use invitations, unique usernames, login/logout, bootstrap/reset | Rust auth/admin integration tests |
| Cookie/Origin authorization, device revocation, 60-second leases | Rust realtime auth and official SDK, including revocation without a worker |
| Unique direct pair, exact lookup, Unicode display names | Rust conversations and real browsers |
| Atomic message/event/outbox, idempotency, exact 64-bit sequence | Rust messages, including 9007199254740993 and rollback |
| Lease reclamation/CAS, predecessor blocking, crash after publish | Rust outbox and actual worker/SDK/browser |
| Snapshot races, duplicate/disordered notifications, delayed selections | Vitest replay and delayed browser snapshots |
| Monotonic receipts, device convergence, no premature hidden/rendered read | Rust receipts and two-device browsers plus delayed replay/render regressions |
| Relationship-only presence, expiring typing, multiple devices | Rust receipts and two actual SDK devices |
| Enter/Shift+Enter/IME, mobile return, drafts/themes, HTML escaping | Chromium UI tests and desktop/mobile inspection |
| Long Unicode messages, history pagination and scroll anchoring | 55 persisted historical messages and Unicode/emoji browser cases |
| Lost HTTP response, offline socket, Valkey/Centrifugo outage/recovery | Browser failure cases with bounded state waits and test-only faults |
| Silent connected dependency cannot block delivery or cancellation | Actual silent TCP server and PostgreSQL worker regressions |
| Foreground restoration recovers missed messages immediately | Browser regression with polling disabled and its original socket retained |
| HTTPS deep links, production cookies/WebSocket, private boundary | Separate Caddy HTTPS deployment |
| Correlation, private metrics, error envelope, trusted proxy | Rust observability/proxy tests and deployment |
| Account/message/revocation backup recovery | Isolated pg_dump/pg_restore with real API login/read |

## Original phase-one result

On 2026-10-13, `./scripts/check.ps1 -IncludeIntegration`, Rust fmt/clippy, Svelte types
(zero errors/warnings), formatting, generated contracts, and static build passed.
Production images were rebuilt with `./scripts/production-test.ps1 -Prepare`; all
`pnpm -C web test:deployment` cases passed. Original logs remain under
`.superpowers/sdd/2026-10-12-phase-1-private-chat/`: `review-full-check.log`,
`review-production-build.log`, and `review-deployment-green.log`.

The original suite contains 72 cases: 36 Rust, 9 frontend units, 6 real SDK, 18 browser
identity/chat/fault, 2 HTTPS, and 1 restore. Identity loss clears session caches.
Messages/drafts are memory-only; localStorage contains theme preference only.
Operator passwords are prompted without echo and are never command-line arguments.

A fresh whole-project review found no Critical issue. Its Important fixes covered
premature receipts for unrendered messages, silent Valkey blocking the worker, and
missing foreground replay. Each regression failed before its fix, then passed with
the complete suite. No Minor findings were deferred in that original review.
See [implementation decisions](implementation-decisions.md) for phase boundaries.

## Architecture refactor

The 2026-10-13 refactor keeps the same contracts and behavior. English conventions,
logical paragraphs and explanatory invariants are recorded in root `AGENTS.md`;
[the architecture guide](../architecture.md) describes code ownership. Boundary
checks are now part of `check.ps1`. Source snapshots and current verification logs
live in `.superpowers/sdd/2026-10-13-code-architecture-and-readability/`.

Final post-review verification on 2026-10-13 passed all 76 cases: 4 architecture
guards, 36 Rust, 9 frontend units, 6 real SDK, 18 browser, 2 rebuilt Linux HTTPS,
and 1 backup restore. Rust fmt/clippy, Svelte types (zero errors/warnings), formatting,
generated API checks and the static build passed. Desktop/mobile screenshots were
inspected after the final component changes. The contract comparison contains only
17 added English operation summaries, with unchanged request/response/status/schema
shapes; the original CSS declaration order remains intact.

Evidence: `review-final-check.log`, `review-production-build.log`,
`review-deployment-green.log`, `contract-differences.json`, `chat-desktop.png` and
`chat-mobile.png` in the refactor workspace. An initial run exceeded the existing
strict typing-expiry assertion by 8 milliseconds; the unchanged assertion passed
in isolation and both subsequent full runs. No test tolerance or product expiry was
changed to obtain a pass.

One fresh architecture review found no Critical issue or concrete runtime regression.
Its application-to-HTTP dependency was fixed with a regression observed RED then
GREEN. Dense component/worker/test examples were expanded and formatter output
checked. Two Minor improvements remain recorded in
[implementation decisions](implementation-decisions.md): broader raw-fetch detection
and relocating the queue row type to remove a small module import cycle.

## Product limits

The deployment is single-node, with no high availability or large-scale load claim.
Phase one excludes groups, attachments/avatars, edits/deletions/reactions/search, PWA,
closed-browser push, and end-to-end encryption. SeaweedFS runs and participates in
joint backup; upload/download and attachment recovery belong to phase two. The server
can read stored cloud messages.

See [development](development.md) to create two real users and
[deployment](deployment.md) for production setup. Random test accounts belong only to
isolated databases and are removed; there are no built-in demonstration credentials.
