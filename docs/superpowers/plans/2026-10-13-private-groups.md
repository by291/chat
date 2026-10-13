# Private Groups Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make private groups usable without losing direct-chat contracts or membership isolation.

**Architecture:** Extend the conversations workflow and existing ordered event/outbox path.
Membership episodes fence every authorization and browser response. Keep the direct
DTO intact and add a group summary variant, with query-cache-owned roster records.

**Tech Stack:** Existing Axum/SQLx/PostgreSQL, Svelte/TanStack Query, Centrifugo.

**Spec:** `docs/superpowers/specs/2026-10-13-groups-design.md`

## Global Constraints

- Root Rust package and `web/`; English code/docs and Chinese product UI.
- Title: 1–100 Unicode characters; `GROUP_MAX_MEMBERS=100`, range 2–1000.
- Invitations: authenticated existing accounts, single use, seven-day expiry.
- The final owner transfers before leaving; ended memberships never grant access.
- Canonical decimal sequences; contiguous replay; no required new direct-chat fields.
- Current-directory/native execution is authorized; commits/pushes await explicit request.

## Review Focus

- Remove/rejoin followed by an old same-ID retry cannot reveal a pre-join message.
- A delayed invalidation or response from an old membership cannot purge/populate a new one.
- Concurrent capacity checks and invite consumption never admit an extra active member.
- A group with no other member still has exactly one summary and cannot lose its owner.
- Filtering at a join boundary never silently advances a notification cursor.

## File Structure

- New migrations `0006_group_memberships.sql`, `0007_group_invitations.sql`.
- `conversations/groups.rs` owns group operations; `group_repository.rs` owns their
  SQL and locking. The existing repository remains the shared message/summary owner.
  Both persistence filenames end in `repository.rs` and neither depends on HTTP.
- Existing conversation DTO/HTTP/service/message/receipt/event modules own shared interfaces.
- Realtime delivery/client/repository own targeted invalidation; no separate aggregate job.
- Chat feature API/store/coordinator/activity/components own group browser behavior.
- Rust, unit, SDK and browser tests prove authority, replay and user workflows.

### Task 1: Membership-safe group API and delivery

**Files:** migrations; `src/conversations/{groups,group_repository,model,http,mod,service,messages,receipts,events,repository}.rs`;
`src/realtime/{delivery,client,repository}.rs`; config/app/types/OpenAPI; `tests/groups_test.rs`.

**Interfaces:**
- Produce `MembershipContext { membership_id: MembershipId, joined_seq: Seq }`.
- Keep `ConversationView` direct; produce untagged `ConversationSummary::Direct(ConversationView) | Group(GroupConversationView)`.
- Group view: `id, kind:"group", title, member_count, membership, my_role,
  latest_event_seq, last_message, unread_count, my_read_seq`.
- Produce `GroupMemberView { membership_id, user:UserView, role, joined_seq, read_seq }`.
- `GroupInput {title}`; group mutations include `membership_id`; rename adds `title`,
  role change adds `role`, transfer adds `user_id`, accept adds `token`.
- Shared send/read/typing and snapshot/event queries add optional `membership_id`.
  Group operations require the active identity; direct operations retain omission.
- Produce `service::require_membership(tx, actor, id, expected) -> Result<Member,AppError>`;
  `Member` includes current membership identity, join/read boundary, kind and role.
- Snapshot/event replies add optional membership context, omitted for direct chats.
- Group durable events: `group.updated`, `membership.updated`; group receipts carry
  membership identity. Targeted control hint: `membership.invalidated` with conversation
  and old membership IDs, no title/message/member content.

- [ ] Write HTTP tests `group_roles_and_owner_transfer`, `join_hides_old_history_and_rejoin_fences_retries`,
  `capacity_and_invitation_consumption_are_atomic`, `mixed_summary_pagination_has_no_duplicates`.
  Assert direct responses unchanged; member/admin restrictions; owner leave conflict;
  new episode after rejoin; hidden preview/unread/history; same-ID hidden retry 409;
  stale send/read/typing membership 409; baseline replay 409; expired/revoked tokens fail.
- [ ] Run `cargo test --test groups_test`; expect failures on missing group endpoints.
- [ ] Add/backfill membership IDs, closed sequence, active pair/owner indexes and title
  with new migrations; retain existing direct joins/read values. Add separate hashed
  group invitations with issuer, expiry, consumption and revocation.
- [ ] Implement endpoint table from the spec, with conversation-first locks and atomic
  event/outbox writes. A transfer swaps roles in one transaction. Returning a saved
  retry rechecks episode visibility. Build one summary per group with visible previews.
- [ ] Extend the existing outbox payload with targeted invalidations; preserve old payload
  decoding defaults and aggregate uniqueness. Fanout uses current members. Add repository
  hydration and optional shared membership DTOs without changing direct JSON output.
- [ ] Run group tests and existing message/receipt/outbox tests; expect zero failures.
  Regenerate contracts with `pnpm -C web api:generate`; never edit generated files.

### Task 2: Group browser workflow and membership fences

**Files:** chat API/models/cache/store/coordinator/realtime/activity; new `GroupDialog.svelte`,
`GroupDetails.svelte`; existing shell/list/timeline/styles; join route; frontend unit/browser tests.

**Interfaces:**
- Consume Task 1 generated summary union, membership context, roster and group operations.
- API adapter exposes create/rename/members/role/remove/leave/transfer/invite/revoke/accept;
  shared snapshot/events/send/read/typing accept optional membership ID.
- Store owns `keys.members(id)` and membership-aware stream generations; coordinator
  owns group dialog/details UI and mutation state. Media later consumes current membership.
- `isGroup(conversation)` discriminates the group shape; direct components retain peer behavior.

- [ ] Add unit regressions `rejoin_resets_stream_and_drops_late_history`,
  `old_invalidation_does_not_remove_new_membership`, `stale_send_and_read_do_not_merge`,
  `group_replay_is_contiguous_above_safe_integer`; run `pnpm -C web test`, expect RED.
- [ ] Adapt typed transport and store. Purge missing groups, replace changed episodes,
  abort/reset histories and roster/activity/pending state, and fence every completion.
  Reestablish a snapshot on a stale join baseline; never advance from a control hint.
- [ ] Implement create/join/details, roster/roles, owner transfer, invitation links,
  group sender names/read counts/multiple typing participants. Preserve localized
  IME/drafts/mobile/scroll and same-user session revalidation behavior.
- [ ] Run unit tests, Svelte checks and formatting; expect zero errors/warnings/failures.

### Task 3: Actual-stack group acceptance

**Files:** group browser/SDK tests and fixtures, operations/architecture/acceptance docs.

**Interfaces:** consume Tasks 1–2 through actual HTTP/WebSocket/browser workflows.

- [ ] Write three-browser creation/join/rename/role/remove/rejoin/transfer/reconnect cases,
  asserting forbidden old history and old-episode hints, visible sender/read state,
  IME/mobile behavior, and recoverable ordered fanout with the actual worker.
- [ ] Run the new cases before claiming acceptance; fix behavioral failures with a
  regression RED→GREEN cycle, not timeout inflation.
- [ ] Run `node scripts/check.mjs --integration`; expect all old and new cases green.
- [ ] Update architecture and operations with actual endpoints, role/join policy and results.
  Carry the group membership interface into media; full rebuilt deployment is validated
  once more after media wiring, avoiding duplicate unchanged image builds.

## Completion

Run a fresh whole-change review after both requested increments, resolve material findings,
verify rebuilt HTTPS wiring, and preserve local evidence. Do not commit/push without the
user's explicit request. No public deployment is part of these tasks.
