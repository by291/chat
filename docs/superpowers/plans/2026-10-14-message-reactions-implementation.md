# Message Reactions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans after execution-method selection. Steps use checkbox syntax.

**Goal:** Add six durable reaction choices with authorized actor-specific projection
and correct replay/multi-device convergence.

**Architecture:** Conversation service/repository operations compose reaction, message
revision, event and outbox under the aggregate lock. The existing cache/state owners
merge authoritative responses; components remain presentational.

**Tech Stack:** Existing Rust/SQLx/PostgreSQL, generated API, Svelte 5, shadcn controls,
Vitest and real Chromium fixtures; no new runtime service.

**Spec:** [Approved reactions design](../specs/2026-10-14-message-reactions-design.md).

**Local status (2026-10-15):** Implemented and verified by the final combined
`node scripts/check.mjs --integration` gate (exit 0). See the
[acceptance record](../../operations/product-readiness-acceptance.md) for source
identity, counts, review follow-up and external checks that remain pending.

## Global Constraints

- Kinds are exactly `like`, `love`, `laugh`, `surprise`, `sad`, `celebrate`, in that display order.
- Each user chooses at most one kind per message; duplicate desired state produces no new event.
- Preserve creation sequence/time, original retry identity, unread counts and read rules.
- Store no actor-specific `mine` flag in durable shared events; project it for each REST reader.
- Reactions do not generate new-message in-page/background notifications; withdrawal hides them.
- Keep existing member-episode authorization, exact decimal sequences and immutable migrations.
- Use [owned verification](2026-10-14-product-readiness-execution.md#targeted-verification-commands); regenerate schemas and keep Chinese UI.

## Review Focus

1. Current-state projection can accidentally retain the mutation actor's `mine`: Task 1 tests all readers and stored payloads.
2. An old HTTP acknowledgement can replace newer counts: Task 2 tests revision fences.
3. Withdrawal can race a reaction request: Task 1 tests serialized terminal state.
4. Rejoining can expose old reaction targets: Tasks 1/3 test episode boundaries.
5. Context/reaction updates can be misclassified as reads/new messages: Tasks 2/3 test cursor and notification behavior.

### Task 1: atomic reactions and complete authorized projections

**Files:** Create `migrations/0015_message_reactions.sql`,
`src/conversations/reactions.rs`, `src/conversations/reactions_repository.rs`,
`tests/reactions_test.rs`.
Modify conversation module/model/message HTTP, `messages.rs`, `message_queries.rs`,
summary projections, router/export and explicit DTO facade exports.

**Interfaces:** Produce `ReactionKind`, `ReactionSummary { kind, count, mine }`,
`SetReactionInput { kind, membership_id }` and
`ReactionUpdateData { message_id, message: Option<MessageView> }`.
Application boundary is
`set_reaction(state, actor, conversation_id, message_id, membership_id, kind: Option<ReactionKind>) -> Result<MessageView, AppError>`;
`None` removes. Repository `set_choice(tx, message_id, user_id, kind)` returns a typed
change outcome, and `summaries(tx, message_id, reader_id)` returns ordered summaries.
Extend current message enrichment to receive the reader actor explicitly on every path.

- [x] Write real HTTP/database tests for idempotent set/remove, switch, two-reader selection, neutral stored event, exact revisions, rollback, withdrawn targets, old episodes and unchanged unread/send retry. Assertion fragments:

```rust
assert_eq!(same_choice_event_count, first_choice_event_count);
assert!(author_view.reactions[0].mine);
assert!(!peer_view.reactions[0].mine);
assert!(stored_reaction_payload.get("message").is_none());
assert_eq!(after.created_seq, before.created_seq);
assert_eq!(after_unread_count, before_unread_count);
```

- [x] Run Rust target `reactions_test`. Expected RED: the additive PUT/DELETE routes do not exist.
- [x] Add constrained reaction persistence and DTO defaults. Expose the approved singular `/reaction` endpoints using current membership extraction; HTTP contains no SQL.
- [x] Lock conversation, authorize current episode/message and reject withdrawal before idempotent success. Only a changed choice updates the message revision and appends one `message.reactions_updated` event/outbox in the same transaction.
- [x] Store `ReactionUpdateData` with only `message_id`; omit its optional message. REST replay fills the current reader's authorized MessageView or substitutes existing MessageUnavailable, so typed event decode remains valid.
- [x] Hydrate reactions across history, snapshots, summaries, search/context, sends/retries, edits/withdrawals and replay. Return `[]` for withdrawals; do not overwrite original send fields or emit push intent for this event.
- [x] Add concurrent switch/remove/withdraw and forced durable-rollback cases. Check bigint revisions beyond JS-safe integers and legacy message/event payload defaults.
- [x] Run reactions/experience/message/receipt/group targets through the owned recipe, generate API and check the contract. Expected GREEN with actor-correct current state everywhere.

### Task 2: typed state actions, replay and accessible controls

**Files:** Modify chat `api.ts`, `models.ts`, `message-actions.svelte.ts`,
`conversation-store.ts`, `message-records.ts`, `realtime.ts`, `MessageBubble.svelte`
and feature styles; create `MessageReactions.svelte` and
`web/tests/unit/message-reactions.test.ts`.

**Interfaces:** Extend `ChatApi` with
`setReaction(id, messageId, kind, membershipId?): Promise<Message>` and
`removeReaction(id, messageId, membershipId?): Promise<Message>`.
State owner produces `reaction(messageId, kind: ReactionKind | null): Promise<void>`
and per-message pending state; it uses existing session/selection/episode fences.
Replay applies `ReactionUpdateData.message` through the existing revision merge.

- [x] Write unit cases for late acknowledgements/history, terminal withdrawal, actor changes, single pending action, replay after context navigation and notification suppression. Assertion fragments:

```ts
expect(merged[0].revision_seq).toBe(newerRevision);
expect(merged[0].reactions).toEqual(newerReactions);
expect(withdrawn.reactions).toEqual([]);
expect(notificationCalls).toBe(0);
expect(readRequestsFromReactionAction).toBe(0);
```

- [x] Run `pnpm -C web exec vitest run tests/unit/message-reactions.test.ts`. Expected RED: the reaction action/replay contract is absent.
- [x] Implement the typed adapter and one pending action per message. Failed HTTP leaves authoritative counts unchanged and shows retryable feedback; stale session/episode responses cannot change cache.
- [x] Add transport validation for the new envelope kind and continuous replay projection. Notification candidates remain restricted to created messages; reaction action itself never confirms DOM visibility/read progress.
- [x] Implement labelled six-choice controls/counts with stable keys, native button types, keyboard focus and current-user pressed state. Disable unavailable/withdrawn targets and preserve composer/IME/scroll behavior.
- [x] Run the unit target, existing chat-experience/notification units and `pnpm -C web check`. Expected GREEN without a second mutable message store.

### Task 3: real devices, migration compatibility and documentation

**Files:** Create `web/e2e/message-reactions.spec.ts`,
`tests/reactions_migration_test.rs`; update chat-experience/architecture/README and
the coordinator plan's acceptance record.

**Interfaces:** Consume the completed DTO/event/revision protocol and existing real
fixture accounts, browser contexts, worker controls and recovery/history helpers.

- [x] Write a populated-schema upgrade regression: messages/receipts/retry hashes remain unchanged, existing messages expose `[]`, and old event payloads decode after adding the new variant.
- [x] Add two-user/second-device browser tests for select/switch/remove, reload/offline replay, keyboard/mobile bounds, withdrawal, delayed response and group removal/rejoin.
- [x] Assert reaction changes leave unread counts unchanged, cannot read a newer unseen message in a context window and generate neither open-page alerts nor push jobs.
- [x] Run Rust target `reactions_migration_test`, the focused units and `pnpm -C web exec playwright test e2e/message-reactions.spec.ts`. Expected GREEN through real SQL/worker/realtime/browser paths.
- [x] Run API check and the coordinator's integrated verification after all focused plans join. Document the API/worker/frontend joint upgrade, stable reaction keys, single-choice behavior and preserved read/notification semantics.
