# Architecture and reading guide

## Reading order

Start with `AGENTS.md` for the conventions, then follow one complete operation:

1. Sending a message: `conversations/messages/http.rs` → `messages/service.rs` → `messages/repository.rs`
   and `events.rs` → `realtime/outbox.rs` → `delivery.rs`.
2. Rendering it: chat `api.ts` → `synchronization/conversation-store.ts` → `sync.svelte.ts`
   → `messages/MessageList.svelte`.
3. Logging in: auth `http.rs` → `service.rs` → `repository.rs` and `password.rs`;
   browser auth `api.ts` → `session.svelte.ts` → `AuthForm.svelte`.

Each file documents its responsibility and the non-obvious invariants. Read the
workflow first and its persistence/transport implementation second.

## Rust ownership

```text
src/
  bin/                          process and CLI composition
  app.rs                        dependency state and router composition
  error.rs                      application errors and HTTP error rendering
  types/                        shared IDs, exact sequence and DTO compatibility facade
  auth/
    http.rs                     cookies, Origin guard and HTTP adapters
    service.rs                  registration, login and session workflows
    repository.rs               user/session SQL and locks
    security/                   credential changes, mailbox enrollment and recovery
      service.rs                security transactions and encrypted challenge intents
      recovery.rs               anonymous recovery and atomic token resets
      repository.rs             credential/email version fences and challenge SQL
    mail/                       encrypted security-mail queue
      worker.rs                 bounded provider delivery and retry lifecycle
      repository.rs             leased queue persistence
    model.rs, validation.rs, password.rs
  friends/
    http.rs, model.rs            contacts and per-person relationship contracts
    service.rs                  consent transitions, policy and transaction ownership
    repository.rs               canonical pair locks, requests and refresh jobs
  conversations/
    http.rs, service.rs          direct creation and authorized summaries
    repository.rs               aggregate/member/event persistence and snapshot setup
    model.rs                    shared conversation/message/event HTTP contracts
    events.rs, receipts.rs      atomic append, monotonic receipts and temporary typing
    messages/
      http.rs                   mutation/search/context REST adapters
      service.rs                immutable sends, edits and withdrawals
      queries.rs                repeatable history/search/context and replay
      repository.rs             message rows, retry metadata and discovery SQL
      reactions.rs              authorized single-choice revision transaction
      reactions_repository.rs   reaction counts and persistence
    groups/
      http.rs                   group REST adapters
      membership.rs             initial membership, roles and membership episodes
      invitations.rs            creation, expiry, revocation and consumption
      repository.rs             group/member/invitation SQL and locks
    preferences/
      service.rs                personal preferences and private refresh hints
      repository.rs             preference persistence and targeted outbox SQL
  notifications/
    http.rs, model.rs            subscription/config/activity REST contracts
    service.rs, repository.rs    session capabilities, binding locks and activity
    jobs/repository.rs          message intent, authorization and lease CAS
    job_repository.rs           explicit compatibility exports for existing consumers
    delivery.rs                 current eligibility and generic provider payload
    worker.rs                   cancellation, expiry and retry orchestration
  users/
    http.rs, model.rs            profile/search/status contracts
    service.rs, repository.rs    profile rules, search and relationship authorization
    presence.rs                 authorized presence and transition maintenance
  media/
    http.rs, model.rs            upload/status/download contracts
    attachment_repository.rs    shared object rows, quota and state locks
    uploads/                    grant allocation, renewal, completion and cancellation
    references/                 authorized downloads and message/avatar attachment binding
    retirement/                 cleanup, reconciliation fencing and single quota release
    processing/
      inspection.rs             bounded image inspection and sanitized variants
      lifecycle.rs              publication, lease and failure transaction decisions
      repository.rs             claims, immutable candidates and processing SQL
    worker.rs                   processing and cleanup orchestration
    service.rs, repository.rs    explicit compatibility exports for existing consumers
  realtime/
    http.rs, model.rs            private Centrifugo HTTP contracts
    service.rs                  authorization and connection session leases
    repository.rs               connection/presence/outbox rows and lease CAS
    delivery.rs                 durable fanout and revoked-client disconnects
    outbox.rs                   cancellation-aware orchestration
    client.rs                   bounded Centrifugo HTTP transport
  infrastructure/               concrete configuration, database and external adapters
```

Workflow directories place related operations beside their SQL and adapters. Their
`mod.rs` files expose intentional entry points; old Rust module paths remain explicit
re-exports where callers rely on them. Small cohesive features stay shallow rather
than acquiring empty layers. Group invitations share membership policy and aggregate
locking with membership operations instead of duplicating either responsibility.

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
`infrastructure::valkey` shares a lazy Redis `ConnectionManager` between rate limits
and presence. Library-owned reconnection preserves the two-second operation deadline;
startup does not require a live Valkey connection, and failed commands are not
automatically retried because an uncertain increment may already have committed.

`infrastructure::environment` uses `config` and Serde for environment sources and
typed settings. Feature configuration retains enabled-service gates, strict scalar
parsing, validation and redacted errors. Existing variable names, defaults and
process-over-dotenv precedence remain unchanged; opaque secrets stay strings.

The realtime repository owns `LeasedJob` because it maps the claimed database row.
Delivery and orchestration consume that record; persistence does not import either
workflow. `outbox` re-exports the type to keep its existing public API compatible.
`realtime::model::OutboxPayload` is the private persisted contract shared by all
job producers and delivery. Its existing JSON tags and fields are unchanged;
historical event jobs still default missing membership invalidations to an empty list.

Outbox cancellation covers claim, fanout, retry and completion, including blocked
SQL. Database phases have bounded deadlines. Completion runs in a caller-owned
transaction so a blocked update can roll back on cancellation; an already accepted
commit can still succeed after confirmed delivery. Lost leases are expected CAS
outcomes, while durable confirmation failures are reported and remain reclaimable.
Push binding expiry likewise belongs to its delivery workflow, composing job and
binding repository operations in one transaction under the existing lock order.

`types` explicitly re-exports DTOs to preserve the existing crate consumers and
OpenAPI names. Internal modules import the specific names they need. The facade
is a compatibility boundary, not the place to add new unrelated DTO definitions.

## Frontend ownership

```text
web/src/
  routes/                       routing, redirects and feature composition
  lib/
    api-client/                 reusable HTTP transport, errors and generated schema
    components/ui/              shadcn-svelte presentational primitives
    styles/                     shared foundation/layout/theme cascade
    features/
      workspace/                authenticated application composition
        lifetime.svelte.ts      identity-scoped effects and foreground reconciliation
        session.ts              feature startup and ordered whole-session teardown
        WorkspaceShell.svelte   application shell and push activity presentation
        WorkspaceNavigation.svelte account identity and primary destinations
      auth/
        api.ts, session.svelte.ts authentication transport and session identity
        recovery/               password recovery/reset forms and transient state
        verification/           email verification form and state
      contacts/
        api.ts, contacts.svelte.ts canonical query observers and consent mutations
        context.svelte.ts       authenticated owner and remembered navigation
        ContactsPage.svelte     list/detail composition and DOM focus/scroll
        page.svelte.ts          URL selection, cancellable search/pagination and open-chat flow
        friends/               friend-list and removal presentation
        requests/              combined directional request projection and presentation
      settings/
        SettingsPage.svelte     settings category composition
        profile/               editable account dialog, avatar and profile form
        security/              email and password forms
        sessions/              device-list query/mutation owner and presentation
        appearance/            saved theme preference and controls
      chat/
        api.ts, models.ts        typed transport and shared application contracts
        cache.ts, context.svelte.ts canonical query keys and application-tree context
        ChatShell.svelte        conversation composition
        sync.svelte.ts          authenticated coordinator and transient UI state
        conversations/         list/actions/overview, summary collection and projections
        messages/              timeline/composer, history, edits and optimistic records
        groups/                group operations, roster cache and dialogs
        synchronization/       private stream generations, continuous replay and transport
        notifications/         open-page policy, live candidate floors and presentation
      media/                    scoped uploads, ephemeral transfers and previews
      notifications/            private background-push capability lifecycle
      pwa/                      install/update browser lifecycle and consent controls
  service-worker/               static cache and generic push/click presentation
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
generations, continuous event cursors and read-acknowledgement flights. Its public
status snapshots are read-only; the coordinator requests reload/read operations
without mutating stream internals. Shared summary/replay flights include error
handling, so concurrent callers observe one consistent outcome. Peer activity owns expiring information.
There is no second mutable copy of authoritative messages.

Settings device lists use TanStack Query observers for request cancellation,
cache subscriptions and revoke results. Their owner still fences logout/navigation,
removes its query on teardown and ignores completed revocations after disposal.
Realtime publications and service-worker push/click payloads use Zod runtime
schemas. Rust/OpenAPI remains the HTTP type source; event validation preserves exact
decimal sequences and notification navigation remains restricted to conversation routes.

Manually maintained canonical records are retained for the authenticated lifetime.
Query-cache subscriptions report changes but do not pin queries as observers;
automatic garbage collection must not remove messages while applied stream cursors
remain live. Membership replacement/removal clears the affected records, and
chat teardown removes only chat-owned query prefixes. The workspace stops all
feature owners before clearing the complete authenticated cache. Settings device-list owners
also abort/fence late responses and remove their own query on teardown.

Pure conversation summary, receipt and preview transforms live in
`conversations/conversation-projections.ts`; `conversations/conversation-summaries.ts` owns paginated collection.
The conversation store retains summary-flight/revision checks, membership episodes
and stream generations. `messages/pending-messages.ts` freezes optimistic send identity and
content while `ChatSync` retains the existing lifetime and acknowledgement fences.
`groups/group-actions.svelte.ts` coordinates group mutations with canonical summary/roster
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

Friendship changes serialize on a canonical pair advisory lock, including absent
rows. Request transitions and two private `contacts.updated` outbox jobs commit
atomically. Existing conversation operations lock their aggregate before pair
locks; friendship-only operations never lock conversations. Sorted group candidate
IDs establish deterministic ordering across multiple pairs. Direct creation locks
its pair before inserting a new aggregate; its existing-row lookup is a plain read
and does not lock or mutate that aggregate. It must not use an `ON CONFLICT`
insertion for existing rows: PostgreSQL can wait for a sender updating that row,
reversing the aggregate-before-pair lock order.

Contacts use one authenticated `ContactsState` and query-cache observers. The
existing personal socket prompts canonical refreshes without advancing any
conversation cursor. Startup, reconnect and foreground reconciliation refresh
contacts. A per-person relationship query gates the direct composer; a partial
friend page is never used as an authorization snapshot. Friend-picker search pages
have active query observers, so relationship invalidation refreshes their results.
Query text and selected chips remain transient UI. Logout cancels queries,
mutations and observers.
Request pages project all participating profiles in one batch within the same
snapshot, preserving request order and pagination without per-row profile queries.

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
  Auth/settings responsive rules live with those features at the former shared
  responsive import position. Shared overrides remain last because their specificity/order is observable.
  `styles/feature-theme.css` preserves the shared dark form-error theme rule
  immediately after chat styles. Feature-specific auth/settings and application-shell
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
Explicit nested workflow API paths are allowed; a nested component named `api.ts`
is not an adapter. Guards also reject repository transaction completion/application
imports, shared dynamic feature imports, and direct whole-cache clearing outside
the workspace. These source checks supplement compiler and behavioral coverage;
aliases and arbitrary dynamic expressions still require review.

`web/eslint.config.js` applies ESLint, TypeScript and Svelte recommended rules to
handwritten application code and tool configuration. Generated schemas are excluded;
test harnesses retain type, format and behavioral gates. Lifecycle bookkeeping maps
remain deliberately nonreactive, and root-deployed application paths do not need
base-path resolution. `pnpm -C web lint` runs semantic checks;
`pnpm -C web format:check` runs Prettier.
`node scripts/check.mjs` includes both alongside the architecture/type/unit/build gates.
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

Public tooling includes `scripts/init-env.mjs` to create local secrets,
`scripts/check.mjs` to run checks, and `scripts/generate-api.mjs` to generate the contract.
The optional root `Makefile` separates Docker dependency lifecycle from background
application lifecycle. `process-compose.yaml` delegates application supervision and
reverse-order shutdown to Process Compose; it does not define containers or install
the supervisor. `scripts/check-dev-infra.mjs` checks dependency readiness before
detaching. Its private socket and logs live in `.local/dev/`, without PID files.
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

## Workspace navigation

The authenticated layout composes `workspace/WorkspaceShell`, which mounts `ChatShell` for both `/app` and
`/app/chats/:id`, keeping its list mounted when selecting conversations.
`workspace/WorkspaceNavigation` is embedded in each workflow sidebar, combining account
identity and primary destinations without a separate navigation rail. Shared
fluid column widths (`clamp(288px, 23vw, 400px)`) and `PaneHeader` geometry keep
workflows aligned without crowding smaller screens. A 240px conversation
overview summarizes canonical profile, preferences, and attachments in loaded
history above 1180px. Its visibility never changes the timeline's state owner.
On phones, details replace the sidebar and expose a top-left parent action.
Feature panes own scrolling. New browsers default to dark appearance; stored
light, dark, and system preferences remain authoritative.

`settings/profile/AccountMenu` opens the centered `ProfileDialog` directly, with editable
profile fields, avatar controls, and logout in one place. Editing one's own profile
never navigates into settings. Settings retains security, notifications,
appearance and application controls. `PeerDetails` and `GroupDetails` stay local to
the conversation, using a right-side panel on desktop and a bottom panel on mobile.
The chat actions menu creates or joins groups; direct chats start from contact details.

Contacts category and selected person are URL query parameters. Context remembers
that destination for primary navigation and stores the list scroll offset, but
Back/Forward and deep links derive selection from the URL. Settings retains its
existing fragment IDs as category destinations and keeps hidden category forms
mounted until the settings route is left; password drafts remain local.
Contacts deep links load canonical pages until the selected record is found.
Friend search observes server-filtered cache pages; request search loads remaining
request pages because that endpoint has no query filter. Route cancellation stops
pagination without cancelling shared cache reads. Incoming and outgoing requests
appear together in the New Friends tab, sorted by creation time and labeled by
direction. Direction determines the available actions, not a separate filter.
Legacy incoming/outgoing deep links resolve to this combined list.

`ChatContext.readingPositions` contains DOM anchors and offsets only, keyed by
conversation and checked against membership identity. `MessageList` restores the
anchor after history is ready; it still independently confirms rendered visibility
before read acknowledgement. Membership changes remount the timeline, and identity
teardown clears remembered positions. Settings return resumes the selected chat;
explicit conversation Back opens `/app` and clears the selection.
