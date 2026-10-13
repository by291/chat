# Phase two: private groups

Date: 2026-10-13. Status: approved for implementation by the user.
This refines the approved wider [product design](2026-10-12-chat-design.md) for the
user's request to implement groups alongside CI and SeaweedFS media. Existing
private text chat remains usable during the additive migration.

## Outcome and scope

Individuals and small teams can create private groups, invite existing accounts,
manage members, exchange durable messages, and see unread/read status across devices.
Success requires three actual browser users exercising those workflows against the
real stack, including removal, rejoin and reconnect. Root Rust, `web/`, English code
and documentation, Chinese UI, cookie/Origin authentication and canonical decimal
sequence strings remain the established conventions.

Create an owner-only group with a trimmed title of 1–100 Unicode characters.
Other users join through invitations rather than being added without acceptance.
`GROUP_MAX_MEMBERS` defaults to 100, counts the owner and active members, and accepts
2–1000. This is a configurable product limit, not a measured capacity guarantee.
Owner-only groups are valid; group deletion/disbanding is outside this increment.

## Roles and invitations

| Operation | Owner | Admin | Member |
| --- | --- | --- | --- |
| Rename, issue/revoke invitations | Yes | Yes | No |
| Remove an ordinary member | Yes | Yes | No |
| Remove/demote an admin, promote a member | Yes | No | No |
| Transfer ownership | Yes | No | No |
| Leave | After ownership transfer | Yes | Yes |

An owner cannot be removed. Transfer promotes an active target and changes the former
owner to admin atomically. The final owner must transfer before leaving, including
an owner-only group. The UI explains that requirement before submission.

Group invitations are distinct from registration invitations, use random hashed
single-use tokens, expire after seven days, and admit only an authenticated existing
account as an ordinary member. Owners/admins can revoke them. Acceptance rechecks
that the issuer is still an active owner/admin, capacity, expiry and consumption.
An already-active account cannot consume an invitation or another capacity slot.
Tokens appear only in the created link, never in logs or retained plaintext rows.
The join route preserves the pending invitation through login and clears it after
successful acceptance. No email or external message delivery is part of this scope.

## Membership episodes and authorization

Add a UUID membership identity to each existing member row, retain ended episodes,
and replace the pair primary key with the membership identity. A partial unique
index permits only one active episode for each conversation/user. Preserve all
existing direct member join/read values during backfill. Add `left_seq`, role
constraints and group title through new migrations; do not rewrite old migrations.

The current active episode alone grants access. A joining member's membership event
has sequence `J`; visible messages/events begin at `J`, with replay baseline `J-1`.
Removal and rejoin create a new identity and boundary. Old history, previews, unread
counts, attachment links and retry IDs do not become visible again. Ended episodes
remain for audit and historical message attribution, not authorization.

All mutations lock the conversation before member/invitation rows. Role changes,
capacity, invitation consumption, sequence allocation, event and outbox writes commit
together. Invitations may first be looked up to identify the conversation, but must
be rechecked under this lock order. No database transaction spans a transport call.
Reads authorized before removal may finish; new requests after removal commit fail.
Clients purge revoked access and fence late responses rather than claiming that
already-delivered information can be recalled.

Group send/read/typing inputs carry the expected membership ID. A stale identity
returns the existing conflict error; a removed nonmember receives the existing
authorization response. Returning an idempotent saved message also checks visibility:
a reused client message ID from before the current join conflicts even with the
same body. Membership/receipt events do not count as unread messages.

## API and contracts

Keep the existing direct creation response and `ConversationView` wire shape.
Introduce `GroupConversationView` and an untagged direct/group summary union for
shared list/get results. Group summaries include `kind: "group"`, title, member
count, current membership identity/join boundary, role, cursor, visible last message,
unread count and own receipt; they do not invent a peer or reuse `peer_read_seq`.
Summary pagination returns each group once and filters previews/unreads by join.

Add these authenticated endpoints under `/api/v1`:

| Method/path | Purpose |
| --- | --- |
| POST `/conversations/groups` | Create the owner-only group |
| PATCH `/conversations/{id}/group` | Rename |
| GET `/conversations/{id}/members` | Current roster, roles, episode IDs and read cursors |
| PATCH `/conversations/{id}/members/{user_id}` | Owner changes admin/member role |
| DELETE `/conversations/{id}/members/{user_id}` | Authorized removal |
| POST `/conversations/{id}/leave` | Leave through the current episode |
| POST `/conversations/{id}/owner` | Transfer ownership |
| POST `/conversations/{id}/invitations` | Issue a single-use join link |
| DELETE `/conversations/{id}/invitations/{invitation_id}` | Revoke |
| POST `/group-invitations/accept` | Consume token and join atomically |

Group snapshots/replay replies add membership context. Requests below the current
join baseline require a new snapshot instead of skipping unseen events. Direct
requests omit the new membership fields and retain existing behavior. Rust owns all
DTOs/events; regenerate OpenAPI and TypeScript through `pnpm -C web api:generate`.

## Realtime and browser state

Keep personal Centrifugo channels and durable REST replay. Each membership/title
mutation is a persisted event using the existing sequence/outbox transaction.
Extend its outbox payload with targeted invalidation recipients for removed episodes;
do not allocate another job for the same aggregate ordinal. Removed devices receive
only a content-free `membership.invalidated` control hint with the old episode ID.
A delayed hint for an older episode cannot purge a new membership.

The query cache owns summaries, roster and messages. Membership replacement aborts
requests and clears that conversation's history, activity, pending operations, rendered
cursor and receipt confirmation. Session and membership fences cover every late
history/replay/send/read result. Summary refresh also removes disappeared groups.
Only snapshots and contiguous persisted events establish/advance applied cursors.

The existing chat workflow gains cohesive create/join and group-details components.
Show group title/count, sender names, multiple typing participants and own-message
read counts derived from current eligible members' receipts; show reader names in
group details. An eligible reader joined no later than the message sequence.
Preserve direct double-check behavior, IME input, drafts, mobile navigation and scroll.

## Ownership and acceptance

Group operations live in `conversations/groups.rs`; DTOs/HTTP stay in the feature's
model/adapter, SQL/locking in its repository. Replay/receipts and realtime delivery
extend their current owners. The frontend stays under `lib/features/chat/` with
feature API adapters and a cached roster, following the architecture map.

Acceptance covers concurrent capacity/invite consumption; owner-transfer/leave races;
atomic rollback; pre-join summary/history concealment; remove/rejoin and stale retry
conflicts; baseline replay above Number.MAX_SAFE_INTEGER; no premature read receipt;
removed-device purge and old-episode invalidation; reconnect/fanout recovery; and
three-user desktop/mobile workflows. Retain all direct-chat behavior regressions.

Run `scripts/check.ps1 -IncludeIntegration`. Rebuild and validate the HTTPS deployment
because routing/events and deployed behavior change. Media uses this same membership
boundary in its subsequent design; groups must be independently usable first.
