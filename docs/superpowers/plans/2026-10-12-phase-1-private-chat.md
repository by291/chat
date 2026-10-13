# Phase one: reliable private chat implementation plan

Status: the user approved inline implementation; all twelve tasks and final
acceptance completed on 2026-10-13. This English edition preserves the implemented
scope, task dependencies and invariants. The subsequent architecture refactor changes
source ownership; current paths are in [the architecture guide](../../architecture.md).

**Goal:** invitation accounts, revocable device sessions and recoverable private chat
across two users and multiple browsers of one user.

**Architecture:** static SvelteKit calls Rust; a PostgreSQL transaction commits message,
event and outbox. A separate worker sends body-free Centrifugo personal-channel hints.
Authorized REST supplies replay/history. Connect/refresh proxies reuse database
sessions; Valkey supplies rates/broker; SeaweedFS is configured for phase-two media.

**Specification:** [approved product design](../specs/2026-10-12-chat-design.md).
The exact pinned stack is recorded in manifests and deployment documentation.

## Global constraints and constants

- Implement in the current directory, Rust at root and frontend in `web/`.
- Chinese responsive product UI; English development documentation/comments after refactor.
- No Git initialization/commit or external publishing without an explicit request.
- Stable `/api/v1` paths, error code/message/request ID, Rust-generated DTOs.
- UUIDv7 entities; canonical decimal-string sequences in `0..=i64::MAX`, BigInt clients.
- Own `personal:#<user_id>` only; hints never carry message bodies.
- HttpOnly, SameSite=Lax, host-only sessions; Secure with production `__Host-` cookie.
- Exact Origin enforcement for mutations and realtime; no session localStorage.
- Username: trimmed ASCII lowercase `[a-z][a-z0-9_]{2,31}`.
- Display name: trimmed 1–64 Unicode scalars; password: 12–128 scalars.
- Message: preserve original whitespace, reject blank or more than 4096 scalars.
- 256-bit session/invite tokens, SHA256 stored hashes; sessions 30 days, invites 7 days/single use.
- Message pages default 50/max 100; events default 100/max 200; summaries max 100.
- Connection lease 60 seconds, online summary polling 60 seconds, typing rate 3 seconds/expiry 5 seconds.
- Group/media/edit/delete/search/PWA/calls are later phases. Render text safely.
- Scripts derive the root from `$PSScriptRoot`; Cargo runs at root, pnpm in `web/`.
- Vite/API/proxy development ports 5173/8080/8081; dependency ports 55432/56379/8000/8333.

## Public contracts

All paths below use `/api/v1`. UUID newtypes, `Actor`, `Seq`, `Created<T>` and generated
DTO names remain stable. Actor identity comes from authenticated cookies only.

| Operation | Input/result |
| --- | --- |
| POST `/auth/register` | RegisterInput → 201 UserView and cookie |
| POST `/auth/login` | LoginInput → 200 UserView and cookie |
| POST `/auth/logout` | 204, durable revocation and cleared cookie |
| GET `/auth/me` | UserView |
| GET `/sessions` | SessionView array, no token/hash |
| DELETE `/sessions/{id}` | 204 for owned session, repeated revocation safe |
| PATCH `/users/me` | Display name → UserView; username immutable |
| GET `/users/by-username/{username}` | Exact UserView, no user enumeration |
| POST `/conversations/direct` | peer_id → ConversationView; 201 new/200 existing |
| GET `/conversations` | cursor/limit → items/next_cursor, stable updated_at/id seek |
| GET `/conversations/{id}` | Authorized ConversationView |
| POST `/conversations/{id}/messages` | client_message_id/body → MessageView; 201 new/200 retry |
| GET `/conversations/{id}/messages` | before/limit → ascending items/next_before/snapshot_seq |
| GET `/conversations/{id}/events` | after/limit → events/last_seq/snapshot_seq/has_more |
| POST `/conversations/{id}/read` | message_seq → monotonic my_read_seq |
| POST `/conversations/{id}/typing` | is_typing → 204, no persisted event |
| GET `/users/{id}/status` | online, self/current peer only |

Private listener: `/internal/centrifugo/{connect,refresh}` with an internal credential.
OpenAPI includes persistent and ephemeral event DTOs.

## Task sequence and completed evidence

Tasks run 1 → 12 because identity, transaction ordering and replay share contracts.
Tests record RED before new behavior, then GREEN. Existing passing checks remain.

### Task 1: reproducible environment and health

- [x] Root package/toolchain/locks, web static configuration, Compose four dependencies,
  random environment initializer and isolated SQLx test fixture.
- [x] `Config::from_env`, `connect_db`, `AppState::new`, `build_router`, and TestApp.
- [x] Health returns live 200 with a closed DB pool, ready 503 without secret leakage.
  Verify Cargo, Svelte types/build, dependency readiness and actual ports.

### Task 2: accounts, devices and CLI

- [x] Identity migration, bounded Argon2 service (four permits), opaque sessions and
  transactional invitation/user/session creation; duplicate username leaves invite usable.
- [x] Atomic Valkey rates: registration 5/IP/minute, login 10/IP and username/minute;
  dependency failure is 503, trusted proxy cannot be spoofed by arbitrary XFF.
- [x] Hidden-input bootstrap once, seven-day invite and reset/revoke-all. Reset's
  exclusive user lock orders against login's shared lock.
- [x] Concurrent invitation, case-insensitive username, Origin, expiry/revocation,
  foreign-session and operator tests. Token/hash never enter public JSON/logs.

### Task 3: profiles and unique private pairs

- [x] Unicode profile validation and exact normalized lookup, no enumeration.
- [x] Canonical unordered pair with unique constraint; concurrent create returns one
  conversation; reject self. Members/event/job commit together.
- [x] Authorized summary seek pages and `require_member`; nonmembers receive 404.
  Verify concurrency, privacy, self rejection and Unicode behavior.

### Task 4: message persistence and replay

- [x] Per-conversation row lock, message/event/outbox transaction, unique sender/client ID.
  Same content returns original; changed content conflicts. Sending rate 120/user/minute.
- [x] Repeatable-read history/event pages; set isolation before the first query. Filter
  membership join boundary; reject future after-cursor; descending DB history is returned ascending.
- [x] Transaction rollback does not skip a sequence. Test exact string
  `9007199254740993`, concurrent retries, event/outbox atomicity and snapshot/replay consistency.

### Task 5: private realtime proxy

- [x] Same-origin WebSocket forwards actual Cookie/Origin HTTP headers, never JSON claims.
  Authenticate database identity; no public connection JWT, metadata or other-user subscription.
- [x] Private connect/refresh credential gate, session-bound client records, 60-second lease.
  Check session validity in the attach write to close the auth/revocation race.
- [x] Rust proxy and actual SDK checks reject missing/bad Origin, expiry/revocation,
  spoofed identity/subscription and refresh metadata; test revocation without worker delivery.

### Task 6: ordered durable worker

- [x] LeasedJob, claim_next/deliver/finish/retry/run_worker; head-of-aggregate predecessor
  blocking, SKIP LOCKED, 30-second leases, token/unexpired CAS and capped 1–60-second jitter.
- [x] Network fanout runs after claim, without a held DB transaction. Current members
  receive envelopes; revoked-session jobs disconnect recorded clients. Retain failed jobs.
- [x] Concurrent workers, lease reclaim, stale completion and crash after publish tested
  with actual worker/SDK/browser. Failpoints compile only under the test feature.

### Task 7: receipts and ephemeral activity

- [x] Lock aggregate, authorize a visible message, advance max receipt only when changed;
  receipt/event/outbox are atomic. Count unread non-own messages, not event deltas.
- [x] Authorize/rate typing, publish expiring hints without persisted history. Presence
  aggregates devices, exposes only relationships, and returns unavailable on dependency error.
- [x] Test backwards/duplicate/foreign receipt, unread counts, typing expiry, relation
  privacy and two-device presence. Worker maintenance and Valkey calls are bounded/cancellable.

### Task 8: generated client and identity UI

- [x] Deterministic Rust OpenAPI export; generate TS and verify byte equality in CI.
  Shared cookie-authenticated client with network/application error normalization.
- [x] Session restore/login/register/reset-on-401/logout; Unicode profile, device revoke,
  validation and duplicate-submit guards. Registration URL contains invite only.
- [x] Browser registration/reload/revocation/profile and auth units; no stored session token.

### Task 9: recoverable chat state

- [x] Subscribe before snapshot; independent applied/target/generation/abort state per
  conversation. Serial contiguous replay, exact BigInt, cache keys, own optimistic ID merge.
- [x] Restore summaries on reconnect/poll/foreground; keep drafts through same-user identity
  revalidation; clear all session cache/socket/timers on logout.
- [x] Controlled tests for snapshot notification, duplicates/gaps, old responses, same-ID retry,
  hidden reads and delayed replay/rendered receipt races. Real SDK tests remain mandatory.

### Task 10: desktop/mobile UI

- [x] Conversation list, keyed timeline/composer, username dialog, responsive navigation,
  empty/loading/error/reconnecting/saved/failed states and explicit retry.
- [x] History anchoring and follow-latest; per-conversation drafts; Enter/Shift+Enter/IME;
  focused/visible rendered-read confirmation; theme-only localStorage.
- [x] Real browser text escaping, navigation, drafts, long Unicode, keyboard/composition,
  visibility, 55-message history and desktop/mobile inspection.

### Task 11: production, metrics and restore

- [x] Separate multi-stage Rust/web Docker contexts, static SPA Caddy, exact proxy boundary,
  trusted address and secure cookie, injected secrets, health and SIGTERM handling.
- [x] Correlated request/error logs, bounded route labels, private authenticated metrics.
  Backlog, age, retry/delivery/reclaim and proxy-failure metrics.
- [x] Custom DB dump plus joint SeaweedFS backup; restore only a new isolated named project.
  Real login/history/revocation recovery; attachment recovery remains phase two.
- [x] HTTPS tests validate deep links, REST/WebSocket, cookies and private endpoints.

### Task 12: complete fault acceptance

- [x] Browser lost-response retry, offline replay, publish/crash duplicate, history/new-message
  race, device receipts and revoked socket/REST/reconnect.
- [x] Valkey outage fails protected writes but leaves history readable; Centrifugo outage
  leaves writes durable for later delivery. Faults target only owned test processes/containers.
- [x] Full checks, actual realtime/browser tests, fresh Linux HTTPS images and isolated restore.
  One fresh final review, one Important fix pass with RED/GREEN and complete revalidation.
- [x] Acceptance/decisions/limitations recorded, local account creation documented; no Git commit.

## Completion

All twelve tasks completed. Original acceptance totals 72 cases and fixes three
Important review findings. [Acceptance](../../operations/phase-1-acceptance.md)
describes evidence and product limits. The architecture refactor has its own
[plan](2026-10-13-code-architecture-and-readability.md) and verification record.
