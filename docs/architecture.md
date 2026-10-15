# Architecture and reading guide

## Reading order

Start with `AGENTS.md` for the conventions, then follow one complete operation:

1. Sending a message: `conversations/http.rs` → `messages.rs` → `message_repository.rs`
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
    security.rs           password changes, verified mailbox enrollment and encrypted intents
    recovery.rs           anonymous verified-mailbox recovery and atomic token resets
    security_repository.rs credential/email version fences and challenge SQL
    mail_repository.rs    encrypted queue persistence and leased completion
    mail_worker.rs        independent bounded delivery and retry lifecycle
    repository.rs         users/invitations/sessions SQL and locking
    password.rs           bounded Argon2 work
  conversations/
    http.rs               REST adapters
    group_http.rs         private group REST adapters
    model.rs              conversation/message/event/pagination DTOs
    service.rs            direct conversation and summary workflows
    messages.rs           immutable send identity, author mutations and content projection
    message_http.rs       message mutation/search/context REST adapters
    message_queries.rs    repeatable history/search/context and authorized replay
    message_repository.rs message rows, immutable retry metadata and discovery SQL
    preferences.rs        actor preferences and private durable refresh hints
    preferences_repository.rs preference persistence and targeted outbox SQL
    receipts.rs           monotonic read receipts and temporary typing
    reactions.rs          authorized single-choice revision/event transaction
    reactions_repository.rs reader-specific counts and single-choice persistence
    events.rs             transaction-composed event/outbox append
    repository.rs         conversation/member/message/event SQL
    groups.rs             roles, membership episodes and single-use invitations
    group_repository.rs   group/member/invitation SQL and locking
  notifications/
    http.rs               authenticated subscription/config/activity adapters
    model.rs              public capability/config acknowledgement DTOs
    service.rs            current session, endpoint capability and activity ownership
    repository.rs         deterministic binding locks and leased activity SQL
    job_repository.rs     transactional message intent, authorization and lease CAS
    delivery.rs           current eligibility and generic provider payload
    worker.rs             independent cancellation, expiry and retry lifecycle
  users/
    http.rs               profile/search/status REST adapters
    model.rs              user/profile/status DTOs
    service.rs            profile rules and compatibility presence exports
    presence.rs           relationship-authorized presence and transition maintenance
    repository.rs         profile and relationship authorization SQL
  media/
    http.rs               authenticated upload/status/download adapters
    model.rs              stable metadata and short-lived transfer grants
    uploads.rs            scoped upload allocation, renewal, completion and cancellation
    references.rs         authorized downloads and transaction-composed message/avatar binding
    processing_lifecycle.rs candidate/publication/lease/failure transaction decisions
    retirement.rs         cleanup/reconciliation fencing and single quota release
    attachment_repository.rs attachment/object rows, common locks and quota/state SQL
    upload_repository.rs  upload attempts, sealing and processing enqueue SQL
    processing_repository.rs processing claims, immutable candidates and lease/job SQL
    reference_repository.rs message/avatar reference and visibility SQL
    retirement_repository.rs retirement discovery, tombstones and scheduling SQL
    service.rs            explicit application compatibility exports
    repository.rs         explicit persistence/worker compatibility exports
    processing.rs         bounded image inspection and sanitized variants
    worker.rs             independent processing, cleanup and retired-key reconciliation
  realtime/
    http.rs               private Centrifugo and metrics HTTP adapters
    model.rs              proxy and realtime publication DTOs
    service.rs            connect/refresh authorization and session leases
    repository.rs         connection/presence/outbox persistence, leased rows and CAS
    delivery.rs           durable fanout and revoked-client disconnects
    outbox.rs             cancellation-aware worker orchestration
    client.rs             bounded Centrifugo HTTP transport and publication compatibility exports
  infrastructure/         configuration, DB setup, rate limiting and observability
```

The request direction is **HTTP adapter → application operation → repository**.
Services own transactions so related writes can commit together. Repositories do
not commit caller-owned transactions or import HTTP adapters. Concrete functions
are sufficient for this single PostgreSQL implementation; adding a trait is not
required to make a boundary clear.

`infrastructure::db` owns bounded acquisition and connection recycling. Cancellation
can interrupt transaction startup before SQLx records its transaction depth, leaving
server-side transaction state on a released connection. Recycling clears that state
after the workflow relinquishes the connection; timeout or cleanup failure discards
the connection. Application operations continue to own commit and rollback decisions.
The reset adds one simple-protocol batch per release to enforce this ownership
boundary even when client-side transaction bookkeeping was interrupted.

Media upload, processing, reference and retirement operations own their transactions.
Concrete repositories accept those transactions and never begin/commit them.
Processing and retirement share quota/attachment/job primitives without importing
one another's workflows. Common helpers preserve statement-level lock ordering:
conversation → owner quota → sorted attachments → job/object where applicable;
lease renewal retains its job → object order and separate READ COMMITTED statements.
Storage/network work stays outside these transactions. The legacy `media::service`
and `media::repository` paths remain explicit compatibility facades.

`users::presence` owns authorization/maintenance, `infrastructure::presence` owns
Valkey transitions, and realtime persistence owns stale connection pruning.
The 120-second connection retention remains separate from transition expiry.

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
    components/ui/              shadcn-svelte presentational controls and primitives
    utils.ts                    class merging and shared UI prop types
    styles/                     shared foundation/layout/responsive/theme cascade
    features/
      auth/                     auth API, session lifetime, identity form and styles
      settings/                 profile/device API, forms, theme and styles
        sessions.svelte.ts      authenticated device-list cache and revoke coordination
      pwa/                      install/update browser lifecycle and consent controls
      notifications/            session-owned background capability and activity lifecycle
      media/                    scoped uploads, ephemeral transfer state and previews
      chat/
        api.ts                  typed feature HTTP adapter
        models.ts               aliases to generated DTOs and application contracts
        cache.ts                query cache configuration and key ownership
        conversation-store.ts   summaries, snapshots, history, continuous replay
        conversation-summaries.ts paginated collection without independent lifetime state
        conversation-projections.ts pure summary/receipt/last-message projections
        message-records.ts      revision fences and independently revised reply previews
        pending-messages.ts     immutable optimistic send/retry records and acknowledgement matching
        group-actions.svelte.ts group mutation and canonical cache reconciliation
        history.svelte.ts       search/context ID windows and latest-history navigation
        message-actions.svelte.ts reply selection, edit drafts and personal preferences
        notifications.svelte.ts open-page alert policy and ephemeral browser capabilities
        activity.svelte.ts      temporary typing/presence and timer cleanup
        sync.svelte.ts          authenticated chat coordinator and UI state
        realtime.ts             Centrifugo SDK and publication validation
        context.svelte.ts       application-tree chat context
        *.svelte                readable, cohesive UI components
        chat.css                feature styling
  service-worker/              static cache composition and generic push/click presentation
```

Routes and components consume feature state/API adapters. Transport is reusable
and feature-independent. A component should not need to understand a replay cursor,
SQL transaction, websocket subscription or typed HTTP URL to display a conversation.

Common controls use the shadcn-svelte component source in `components/ui/`.
This shared layer depends on Bits UI, styling utilities and Svelte, and never imports
feature state or transport. Forms, avatar authorization and blob URL lifetimes stay
with their features. The UI migration does not change the query cache, coordinator,
replay, uploads or session ownership.

`styles/ui-theme.css` maps the product palette to shadcn semantic tokens. Product
colors use `--brand` and `--subtle-text` so the library's `--accent` and `--muted`
surface tokens remain distinct. The existing `data-theme` attribute also drives
Tailwind's dark variant, including dialogs portaled beneath the document root.
See [UI components](development/ui-components.md) for component commands and local
adaptations.

The query cache owns authoritative server records. The coordinator owns selection,
drafts and optimistic sends. The conversation store owns per-conversation request
generations and continuous event cursors. Peer activity owns expiring information.
There is no second mutable copy of authoritative messages.

Manually maintained canonical records are retained for the authenticated lifetime.
Query-cache subscriptions report changes but do not pin queries as observers;
automatic garbage collection must not remove messages while applied stream cursors
remain live. Membership replacement/removal clears the affected records, and
logout/coordinator teardown clears the session cache. Settings device-list owners
also abort/fence late responses and remove their own query on teardown.

Pure conversation summary, receipt and preview transforms live in
`conversation-projections.ts`; `conversation-summaries.ts` owns paginated collection.
The conversation store retains summary-flight/revision checks, membership episodes
and stream generations. `pending-messages.ts` freezes optimistic send identity and
content while `ChatSync` retains the existing lifetime and acknowledgement fences.
`group-actions.svelte.ts` coordinates group mutations with canonical summary/roster
refresh; group components retain inputs, dialogs and presentation.

Background capabilities are private acknowledgements owned by one authenticated
notification lifecycle. They never become message records or persistent browser
storage. The root PWA owner tracks install/update state independently of identity;
the worker uses `api-client/asset-cache.ts` with an explicit public asset allowlist.

Account mail and Web Push use independent worker loops. Security mail stores only
encrypted payloads and version-fenced challenge intent. Push intent shares the new
message transaction, then rechecks session/generation/member/read/mute state outside
that transaction. Provider calls run without SQL locks and have bounded deadlines.
`infrastructure/mail{,_config}.rs` owns SMTP/TLS and queue encryption;
`infrastructure/web_push.rs` owns pure-Rust encryption/signing and pinned public DNS
transport, with startup validation under `web_push/config.rs`.

Every message source uses the same `revision_seq` merge, including HTTP mutation
acknowledgements, summaries, snapshots, search/context pages and durable replay.
Reply previews carry the target's separate revision: editing or withdrawing a
target refreshes cached quotes without changing the replying message's revision.
Search and context state retain IDs into the canonical message cache rather than
copies of records. Their requests are fenced by selection, session lifetime and
membership episode. Context windows suppress read acknowledgements until an
explicit return to the latest rendered timeline.

Live message envelopes identify bounded notification candidates. Continuous replay
must apply the exact candidate before announcing it; initial snapshots, replay
backlog and reconnect reconciliation remain silent. Current-chat focus is separate
from bottom-of-timeline visibility, so scrolling history neither triggers alerts
nor acknowledges unseen latest messages. User gestures enable sound and system
permission; system alerts contain no message bodies and open their conversation.
The authenticated coordinator closes browser capabilities and clears alerts on
logout. Its layout renders the in-app alert center on every authenticated route,
including settings and group invitations. Startup/reconnect floors distinguish
backlog from live candidates; metadata initialization retains unknown candidates
and adopts their authorized membership before continuous replay. Personal
preference hints invalidate/refetch summaries without advancing
the conversation cursor. See [chat experience](operations/chat-experience.md).

Media uploads keep files, transfer progress and abort controllers in the authenticated
lifetime. Stable attachment metadata belongs to the query cache. New membership
episodes discard related drafts and fence late transfers. Signed URLs are ephemeral
capabilities; they never become message fields or persistent cache records.

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
- Media retains immutable retired-key tombstones. The worker revisits at most 50
  oldest-due keys per pass on an hourly schedule, protecting current references and
  releasing quota only during the original confirmed retirement.
- UI animation and semantic theme layers load before workflow styles in `app.css`.
  Feature styles and shared styles retain their relative cascade order.
  Shared overrides remain last because their specificity/order is observable.
  `styles/feature-theme.css` preserves the shared dark form-error theme rule
  immediately after chat styles. Feature-specific auth/settings and application-rail
  selectors live with their owners.

## Verification

`node --test tests/architecture.test.mjs` checks the SQL/HTTP and frontend dependency
boundaries without imposing arbitrary file-size rules. Frontend checks inspect
TypeScript plus Svelte scripts, template expressions and custom-element options.
Typed methods and direct `fetch`/browser-global calls are restricted to feature APIs
and shared transport.
Rust source inspection includes new feature directories automatically, while
startup composition and HTTP adapters retain their legitimate adapter dependencies.
Static bracket access, optional calls and expression wrappers are covered; comments,
strings and unrelated object methods are ignored. Aliases and dynamic property access
still require review; the guard is not a complete dependency analysis.
`node scripts/check.mjs` includes those guards and normal format/type/lint/unit/build checks.
Integration checks retain
real PostgreSQL, Valkey, Centrifugo, SeaweedFS and browsers; they cover the invariants
above rather than mocking the end-to-end application.

The root `compose.yaml` owns local infrastructure. Development uses `chat-dev`;
test launchers use `chat-test` with separate loopback ports, credentials and volumes.
Rust tests create unique databases and storage buckets in that test instance.
Browser/SDK tests run native application processes against the same instance;
restore drills reuse the same Compose file under a fresh `chat-restore-*` project.
Production service requirements live in `docs/operations/deployment.md` and do not
prescribe Compose. HTTPS browser checks target an operator-provided staging address.

Public tooling has three entry points: `scripts/init-env.mjs` creates local secrets,
`scripts/check.mjs` runs checks, and `scripts/generate-api.mjs` generates the contract.
`tests/architecture-rules.mjs` owns source boundary inspection.
`tests/infrastructure.mts` owns dependency-free middleware configuration and argv
execution. It can serve Rust checks without importing frontend packages.
`web/tests/integration/runtime.ts` owns native child processes,
unique fixture databases/buckets and cleanup. Browser fixtures share this owner.
`recovery.ts` coordinates a fixture snapshot and restores it into a fresh project;
it is a recovery test helper. Production backup procedures remain operator-owned.
Test logs and protected configuration live in ignored `.local/tests/` directories;
there are no persisted process IDs or dated scratch paths in the tooling.

CI runs backend, frontend and contract/integration scopes on three independent
runners. Local full checks remain serial; tests within a scope retain fixed-port
ownership. See `docs/operations/continuous-integration.md` for scope commands,
cache ownership and Dependabot updates.

The original phase-one acceptance remains in `docs/operations/phase-1-acceptance.md`.
Private group episodes, media limits and deployment configuration are documented in
`docs/operations/groups-and-media.md`. Summary authorization, latest-message metadata
and unread counts share one repeatable-read transaction, so an old episode cannot
be paired with content committed after removal or rejoining.
