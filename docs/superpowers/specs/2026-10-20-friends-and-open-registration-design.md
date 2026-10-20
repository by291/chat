# Friends and open registration

Status: approved on 2026-10-20, including disposable development data.

## Intent and confirmed decisions

Replace invitation-gated account creation and arbitrary-user conversation creation
with open registration and an explicit friendship workflow.

- Anyone can register without a registration invitation.
- Friend requests require the recipient's acceptance.
- An accepted friend can be added directly to a group without another acceptance.
- The project has no data that needs preservation. Existing development data may
  be discarded; no legacy account or conversation migration is required.
- Preserve current uncommitted work and do not commit or deploy this change without
  a separate instruction.

The product policy defaults below are included in the approved design.

## Product model

An account, a friendship and a conversation are separate records. Registering does
not make anyone a friend. Searching for a user does not create a conversation.
Accepting a friend request creates the mutual friendship; a conversation is created
or reused when someone chooses to chat.

Use three distinct entry points in the existing plus menu:

- Add friend: search accounts by username or display name, inspect the result and
  send an individual request.
- Start conversation: select accepted friends; one person opens a direct chat,
  multiple people create one group.
- Join group: retain the existing group invitation link/token workflow.

Add a contacts destination to the application navigation. It contains the friend
list and incoming/outgoing requests. Pending incoming requests have a count badge.
Group details gain an Add members action using the same friend picker, excluding
current members. Existing chat selection chips, keyboard behavior and mobile
layouts remain reusable.

Registration invitations and group invitations are different capabilities. Remove
the former completely from active application behavior; preserve the latter as an
additional explicit-consent path for joining a group.

## Registration

Keep the registration endpoint and its cookie/session behavior. Remove the
invitation request field, form field, URL prefill and invitation-specific copy.
Keep username/password validation, optional nickname with username fallback,
duplicate-username handling and the existing registration rate limit.

Remove the admin Invite command and registration invitation creation/consumption
operations. Bootstrap is no longer required for onboarding; retain it as an
optional first-account CLI helper with updated wording.

Remove the obsolete registration invitation table from the fresh-install schema.
Existing development databases may be recreated to use the revised migrations;
do not add an upgrade path solely to preserve disposable development data.
Update generated contracts, operator documentation, registration fixtures and
deployment checks together. Limit any reset to this project's identified
development/test resources.

## Friendship lifecycle

Provide pending, accepted, declined and cancelled request outcomes. Only the
recipient may accept or decline; only the sender may cancel a pending request.
Reject self requests and unknown targets. Sending a duplicate request returns
the existing pending outcome rather than creating another row.

For crossed requests, surface the existing incoming request for explicit
acceptance rather than silently making the users friends. After declining or
cancelling, a new request may be created subject to rate limits.

A friendship is one canonical unordered user pair with a uniqueness constraint.
Either user can remove it. Removal stops future direct messages and future
friend-based group additions, but preserves existing conversation history and
shared groups. Re-accepting a later request reuses the existing direct conversation.
Request records do not act as authorization after friendship removal.

This first version excludes friend remarks, tags, address-book import, public
group discovery, blocking and custom request messages.

## Conversation and group authorization

Require an active friendship when creating/reopening a direct chat and when
writing a new direct message. Historical reads and read receipts remain available
to existing conversation members. Show an Add friend action and disable the
composer when the relationship is absent.

Preserve idempotent acknowledgements of messages already committed before a
friendship was removed. New sends must not bypass friendship checks using old
conversation IDs. Existing edit/withdraw rules for previously sent messages stay
unchanged.

Group creation validates all selected users as the creator's accepted friends.
Any current group member may add their own accepted friends; group-management
actions such as removal, promotion and ownership transfer retain existing roles.
Being a member of the same group does not create a friendship.

Adding members is atomic: validate actor membership, friendships and capacity,
then write all new membership episodes and durable events in one transaction.
Deduplicate requested users and ignore already-active members. Invalid targets
or capacity overflow leave the group unchanged. Rejoining users get a new episode
and cannot read messages from before the new join point.

Existing group invitations retain their current manager permissions and do not
require friendship. Removing a friend does not remove that person from a group.

## Ownership and consistency

Add a cohesive backend friends feature with model, HTTP, service and repository
boundaries. It owns requests, mutual relationships and relationship-change
notifications. Conversations consume its authorization operations rather than
querying friendship tables in HTTP adapters.

Serialize request transitions and friendship mutations per canonical user pair,
including when no friendship row exists yet. Acquire multiple pair locks in
sorted order. Conversation operations lock the conversation before friendship
pairs; friendship-only operations never acquire conversation locks. Keep
friendship validation inside the message/group transaction so concurrent removal
has a defined order relative to sends and additions.

Friendship changes and personal notification jobs commit together using the
existing outbox. Extend personal-channel events with a contacts invalidation
signal. It prompts a canonical REST refresh; it must not advance conversation
replay cursors. Refresh on session initialization and reconnect so dropped
notifications cannot permanently hide a request or stale friendship.

On the frontend, a contacts feature owns typed API adapters, query keys and
session-scoped observers. The server-backed query cache owns friend/request
records; local runes own picker selections, search drafts and pending actions.
Logout aborts requests and clears the relevant cache. Reuse the existing
authenticated realtime connection instead of opening another socket.

All account search remains authenticated and bounded. Friend-request mutations
are rate-limited. Public profile results must not expose email addresses or
other private account fields.

## Fresh-project setup

Start from the new registration and friendship model. Do not backfill friendships
from old conversations, grandfather non-friend messaging, or add legacy-account
onboarding and migration UI. Test fixtures create accounts through open
registration and explicitly establish friendships where required.

Discarding pre-change development data does not change normal product behavior:
after launch, removing a friend still preserves the participants' message history
and shared groups as specified above. Preserve source changes and configuration;
permission to discard application data is not permission to reset the Git worktree.

## Alternatives considered

1. Recommended: real mutual friendships with server-side authorization. This
   establishes the requested social model across registration, messaging and groups.
2. Contacts as a personal bookmark list, while anyone can message or add anyone.
   Smaller change, but it does not implement accepted friendship semantics.
3. Require confirmation for every group addition. This adds another invitation
   lifecycle and does not match the user's confirmed direct-add preference.

## Verification and completion

- Open registration succeeds without a token, retains session issuance and nickname
  fallback, and handles concurrent duplicate usernames correctly.
- Request authorization, crossed/duplicate requests, acceptance/cancellation races,
  removal and re-acceptance have database-backed regression coverage.
- Direct-message authorization cannot be bypassed using existing conversations;
  committed-message retries and historical reads retain their contracts.
- Creation and later group additions enforce friendship, capacity, episode
  history boundaries and all-or-nothing writes under concurrent changes.
- Contacts converge after reconnect; stale responses cannot overwrite a newer
  session or relationship transition.
- Browser coverage exercises registration, request acceptance, direct chat,
  multi-friend group creation, adding members, removal and mobile/keyboard flows.
- Run `node scripts/check.mjs --integration`, regenerate OpenAPI from Rust, and
  inspect relevant desktop/mobile screenshots.
- No production deployment is included. Staging deployment checks apply only
  if a later instruction requests deployment.
