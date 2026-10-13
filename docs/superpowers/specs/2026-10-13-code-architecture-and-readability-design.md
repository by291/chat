# Code architecture and readability design

## Intent and authorization

The user requested a complete Rust/frontend readability and architecture refactor,
with enforceable development conventions in `AGENTS.md`. Explicit priorities are
logical blank lines and necessary comments. The user also specified English for
all code comments and repository documentation. The work is authorized in the current
directory; it introduces no new product feature or infrastructure service.

## Chosen approach

Retain the modular monolith and the existing frontend features, and separate concrete
responsibilities inside them. A formatting-only change would leave SQL, HTTP, and
state ownership entangled. A generic Clean Architecture framework would add traits,
wrappers, and navigation overhead without multiple implementations. Cohesive feature
modules with a clear dependency direction address the actual problems with less code.

## Rust responsibilities

HTTP adapters delegate to application operations. Application operations own input
rules, authorization and transaction boundaries. Repositories own SQL and locking;
feature models own DTOs. Shared IDs/actor/sequence and existing type re-exports remain
under `types`. Authentication gains a repository and validation module; conversations
separate summaries, messages/replay, receipts/typing, and atomic event append. Realtime
separates proxy contracts, proxy operations, queue persistence, delivery, and the worker
loop. User profile/presence SQL moves into its repository. Binaries only wire/start/call.

## Frontend responsibilities

Keep auth/chat/settings as cohesive features. Typed transport/error handling lives in
`api-client`. Feature API adapters translate application requests into HTTP calls.
The chat coordinator owns session lifecycle and UI state; the conversation store owns
authoritative cached records, snapshots, continuous replay and request generations;
peer activity owns ephemeral typing/presence timers. The coordinator retains optimistic
send and rendered read semantics. Components remain readable and consume those boundaries.
Feature and shared styles are separated while preserving cascade order.

## Invariants

- Rust stays at the root; frontend stays in `web/`. No dependency/version upgrade.
- Preserve all public REST/proxy paths, JSON shape, error codes and localization.
- Preserve schema, migration files, transaction isolation/locking, idempotency, outbox
  ordering/CAS, session revocation, Origin/cookie rules and dependency deadlines.
- Preserve exact decimal sequences/BigInt handling, generation fencing, visible reads,
  pending retry IDs, draft lifetime, IME, mobile navigation and history scroll anchoring.
- English documentation/comments; meaningful phase separators; no gratuitous abstraction.
- No Git initialization, commits, publishing, or modification of live application data.

## Verification

Save an original source/documentation snapshot because the directory has no Git history.
Keep the current behavior suite. Add architecture checks that fail on SQL outside a
repository, raw HTTP in components, and shared/frontend dependency inversions. Run
Rust/web checks incrementally, then the full real integration/browser/restore suite,
the rebuilt HTTPS deployment and one fresh final review. Compare generated API structure
with the original, allowing documentation changes only. Preserve evidence and the local app.
