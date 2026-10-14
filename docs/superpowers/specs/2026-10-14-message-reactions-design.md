# Message reactions design

**Status:** Written design approved by the user on 2026-10-14, following the confirmed
roadmap. This feature retains existing message creation, authorization and replay rules.

**Goal:** Add a small, durable reaction choice to messages with correct multi-device
state, without changing unread message counts or generating new-message notifications.

## Product contract

`ReactionKind` has six stable wire values: `like`, `love`, `laugh`, `surprise`, `sad`
and `celebrate`. The UI maps them to their familiar emoji, with Chinese accessible
labels. Each user chooses at most one kind per message; selecting another switches
it, and an explicit removal clears it. Display counts and the current user's choice.
Do not add arbitrary emoji, a reactor-directory dialog or reaction notifications.

Withdrawn messages show no reactions and accept no reaction changes. Remaining group
members retain historical aggregate counts when a reacting member leaves. A removed
or rejoined member cannot access or mutate a message outside their current episode.

## Ownership and interfaces

`conversations/reactions.rs` owns authorization, ordering and transaction composition;
`reactions_repository.rs` owns SQL. `message_http.rs` remains the HTTP adapter;
`model.rs` owns DTO/event definitions. Existing message projection, query and cache
owners integrate reactions rather than adding a parallel message store.

Under `/api/v1/conversations/{id}/messages/{message_id}/reaction`:

| Operation | Input | Result |
| --- | --- | --- |
| `PUT` | `SetReactionInput { kind, membership_id? }` | `200 MessageView` projected for the actor |
| `DELETE` | Existing-style optional membership query | `200 MessageView` projected for the actor |

`MessageView.reactions` is an array of `ReactionSummary { kind, count: u32, mine: bool }`,
ordered by the enum order above. Omit zero-count kinds. Add a serde default for legacy
payloads; new responses consistently return the field, including `[]` for withdrawal.
Peer identity and original send fields remain unchanged. Generate contract files from Rust.

Invalid kinds retain `400 invalid_input`; unavailable/foreign/old-episode targets
retain existing authorization errors. Withdrawn accessible targets return `409 conflict`.
Check authorization before idempotent success. Repeating the same desired state makes
no new revision/event; removing an absent reaction behaves the same way.

## Persistence and transaction rules

Add one immutable migration for `message_reactions`: message ID, user ID, kind,
creation/update metadata and a primary key on `(message_id, user_id)`. Constrain kind
values and retain foreign keys. Do not rewrite existing migrations.

Lock conversation first, then verify the actor's current episode and accessible message.
This uses the same aggregate serialization as editing, withdrawal and receipts.
Set/switch/delete, message revision update, conversation event and outbox commit in
one application-owned transaction. A failure rolls back reaction and sequence changes.

Use event kind `message.reactions_updated` and payload variant
`MessageReactionsUpdated(ReactionUpdateData { message_id, message: Option<MessageView> })`.
Reaction changes advance conversation event sequence and `message.revision_seq`, but
never `message.created_seq`, creation time, original submission hash/attachments or
read/unread ordering. Do not count reactions as new messages or alter summary text.

Stored reaction events carry `message_id` with `message` absent; never store a specific
actor's `mine` flag in shared durable content. This form still decodes through the
typed event enum. Authorized REST replay fills `message` with the latest MessageView,
including reaction counts and the requesting actor's selection; inaccessible targets
become the existing `MessageUnavailable` payload. All history, summary, snapshot,
search/context and replay sources use the same projection and revision merge.

Withdrawal projection returns empty reactions regardless of historical rows/events.
Historical reaction rows may remain for integrity; they are not a source of withdrawn
content. Reply previews continue to reflect body/withdrawal; reaction changes do not
turn them into a second reaction UI or an independently mutable message record.

## Browser behavior

Extend the chat adapter and state owner with set/remove actions. Components call that
owner; they do not issue HTTP or manage replay. Fence pending results by authenticated
lifetime, selected conversation and membership episode, and apply revision-aware
merging to acknowledgements exactly as for edits.

Use explicit set/remove operations and one pending action per message to avoid
ambiguous toggles under fast repeated clicks. Failed operations leave authoritative
counts intact and show a retryable error. Realtime publication only raises the replay
target. Add the new event to transport validation and continuous replay handling.

Keyboard/mobile controls use existing shadcn primitives, stable keys, native button
types, labelled choices and pressed state for the actor's selection. Preserve IME,
drafts, scrolling, contextual history and rendered-read confirmation. Existing in-page
and background notification owners ignore reaction events for new-message alerts.

## Acceptance

Real PostgreSQL tests cover duplicate set/remove, concurrent switching, rollback,
foreign/old-episode requests, withdrawal races and exact sequences beyond JS safe
integers. Verify neutral durable payloads and actor-specific projections across all
read paths. Test unread counts and original same-ID message retry after reaction changes.

Frontend tests cover stale acknowledgements/history against newer revisions, withdrawn
messages, pending-action fencing and notification suppression. Real browsers cover
two users and a second device, reconnect/reload, keyboard/mobile interaction and group
removal/rejoining. Run generated API checks and the full root integration entry point.
Deploy API/worker/frontend together for the new event, with old-schema backup and
explicit migration compatibility checks.
