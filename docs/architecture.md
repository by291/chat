# Architecture and reading guide

## Reading order

Start with `AGENTS.md` for the conventions, then follow one complete operation:

1. Sending a message: `conversations/http.rs` → `messages.rs` → `repository.rs`
   and `events.rs` → `realtime/outbox.rs` → `delivery.rs`.
2. Rendering it: chat `api.ts` → `conversation-store.ts` → `sync.svelte.ts`
   → `MessageList.svelte`.
3. Logging in: auth `http.rs` → `service.rs` → `repository.rs` and `password.rs`;
   browser auth `api.ts` → `session.svelte.ts` → `AuthForm.svelte`.

Each file documents its responsibility and the non-obvious invariants. Read the
workflow first and its persistence/transport implementation second.

## Rust ownership

```text
src/
  bin/                    process and CLI composition
  app.rs                  dependency state and public router composition
  error.rs                application errors and stable HTTP error rendering
  types/                  shared IDs, actor and exact sequence; DTO facade
  auth/
    http.rs               cookies, Origin guard, extraction and HTTP results
    model.rs              request/response DTOs and authenticated session result
    validation.rs         username and display-name rules
    service.rs            invite, login, bootstrap, reset and revocation workflows
    repository.rs         users/invitations/sessions SQL and locking
    password.rs           bounded Argon2 work
  conversations/
    http.rs               REST adapters
    model.rs              conversation/message/event/pagination DTOs
    service.rs            direct conversation and summary workflows
    messages.rs           send, snapshots and event replay
    receipts.rs           monotonic read receipts and temporary typing
    events.rs             transaction-composed event/outbox append
    repository.rs         conversation/member/message/event SQL
  users/
    http.rs               profile/search/status REST adapters
    model.rs              user/profile/status DTOs
    service.rs            profile rules and relationship-based presence
    repository.rs         profile and presence relationship SQL
  realtime/
    http.rs               private Centrifugo and metrics HTTP adapters
    model.rs              proxy request/reply DTOs
    service.rs            connect/refresh authorization and session leases
    repository.rs         connection/outbox persistence, leased row mapping and CAS
    delivery.rs           durable fanout and revoked-client disconnects
    outbox.rs             cancellation-aware worker orchestration
    client.rs             bounded Centrifugo HTTP transport and event DTOs
  infrastructure/         configuration, DB setup, rate limiting and observability
```

The request direction is **HTTP adapter → application operation → repository**.
Services own transactions so related writes can commit together. Repositories do
not commit caller-owned transactions or import HTTP adapters. Concrete functions
are sufficient for this single PostgreSQL implementation; adding a trait is not
required to make a boundary clear.

The realtime repository owns `LeasedJob` because it maps the claimed database row.
Delivery and orchestration consume that record; persistence does not import either
workflow. `outbox` re-exports the type to keep its existing public API compatible.

`types` explicitly re-exports DTOs to preserve the existing crate consumers and
OpenAPI names. Internal modules import the specific names they need. The facade
is a compatibility boundary, not the place to add new unrelated DTO definitions.

## Frontend ownership

```text
web/src/
  routes/                       routing and application composition
  lib/
    api-client/                 typed HTTP client, shared errors, generated schema
    styles/                     shared foundation/layout/responsive/theme cascade
    features/
      auth/                     auth API, session lifetime, identity form and styles
      settings/                 profile/device API, forms, theme and styles
      chat/
        api.ts                  typed feature HTTP adapter
        models.ts               aliases to generated DTOs and application contracts
        cache.ts                query cache configuration and key ownership
        conversation-store.ts   summaries, snapshots, history, continuous replay
        activity.svelte.ts      temporary typing/presence and timer cleanup
        sync.svelte.ts          authenticated chat coordinator and UI state
        realtime.ts             Centrifugo SDK and publication validation
        context.svelte.ts       application-tree chat context
        *.svelte                readable, cohesive UI components
        chat.css                feature styling
```

Routes and components consume feature state/API adapters. Transport is reusable
and feature-independent. A component should not need to understand a replay cursor,
SQL transaction, websocket subscription or typed HTTP URL to display a conversation.

The query cache owns authoritative server records. The coordinator owns selection,
drafts and optimistic sends. The conversation store owns per-conversation request
generations and continuous event cursors. Peer activity owns expiring information.
There is no second mutable copy of authoritative messages.

## Consistency and lifecycle

- A realtime publication raises a target; only contiguous REST events advance the
  applied cursor. Use BigInt for every sequence comparison.
- Every snapshot/replay/history response checks its conversation generation and
  session lifetime before changing cached state. Switching conversation aborts old work.
- A successful send can arrive ahead of replay. An acknowledgement requires both
  continuous application and the cursor that the UI confirmed after rendering.
- Same-user identity revalidation preserves the coordinator and unsent drafts.
  Logout/revocation tears down sockets, pending work, timers and session caches.
- Typing/presence may expire; durable messages and revoked sessions use the outbox.
  Ephemeral maintenance is bounded and cannot indefinitely block durable jobs.
- Feature styles and shared styles retain the original cascade order in `app.css`.
  Shared overrides remain last because their specificity/order is observable.

## Verification

`node --test tests/architecture.test.mjs` checks the SQL/HTTP and frontend dependency
boundaries without imposing arbitrary file-size rules. Frontend checks inspect
TypeScript plus Svelte scripts, template expressions and custom-element options.
Typed methods and direct `fetch`/browser-global calls are restricted to feature APIs
and shared transport.
Static bracket access, optional calls and expression wrappers are covered; comments,
strings and unrelated object methods are ignored. Aliases and dynamic property access
still require review; the guard is not a complete dependency analysis.
`scripts/check.ps1` includes those guards and normal format/type/lint/unit/build checks.
Integration checks retain
real PostgreSQL, Valkey, Centrifugo, SeaweedFS and browsers; they cover the invariants
above rather than mocking the end-to-end application.

The existing phase-one product scope and deployment limits remain in
`docs/operations/phase-1-acceptance.md`. This refactor changes readability and ownership,
not the feature roadmap, storage schema or wire protocol.
