# Friends and Open Registration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans or superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Allow public registration, require accepted friendships for direct
messaging, and let group members add their own friends directly.

**Architecture:** Introduce a backend friends feature and a frontend contacts
feature. Reuse existing conversation transactions, membership episodes, the query
cache and personal realtime subscription. Keep registration invitations separate
from retained group invitations.

**Tech Stack:** Rust, Axum, SQLx/PostgreSQL, Svelte 5/SvelteKit, TanStack Query,
Bits UI/shadcn-svelte, Centrifugo and Playwright.

**Spec:** `docs/superpowers/specs/2026-10-20-friends-and-open-registration-design.md`

## Global Constraints

- Anyone can register without a registration invitation.
- Friend requests require the recipient's acceptance.
- An accepted friend can be added directly to a group without another acceptance.
- The project has no data that needs preservation; no legacy migration is required.
- Preserve current uncommitted work. Do not commit, push or deploy without a separate instruction.
- Keep Rust at the root and SvelteKit in `web/`; follow `docs/architecture.md`.
- English identifiers, comments and documentation; preserve Chinese product copy.
- Generate OpenAPI and TypeScript schemas from Rust; never edit generated files manually.
- Use existing UI primitives and feature API adapters, with session-scoped cache ownership.
- Reset only positively identified project development/test databases if needed.

## Review Focus

- A late request-accept action after cancellation must not accept a newer request.
- A friendship removal racing a send/add-member operation must have a consistent order.
- A committed message retry after friendship removal must return its original result.
- A removed member added again must not recover a previous membership episode's history.
- A delayed contacts fetch after logout/reconnect must not restore stale permissions or badges.

## File Ownership

| Area | Files and responsibility |
| --- | --- |
| Registration | `src/auth/{model,service,repository}.rs`, `src/bin/admin.rs`, `migrations/0001_identity.sql`, `web/src/lib/features/auth/AuthForm.svelte`: remove registration invitations |
| Friends | New `src/friends/{mod,model,http,service,repository}.rs`, `migrations/0016_friendships.sql`: requests, mutual pairs, authorization and outbox writes |
| Conversation policy | `src/conversations/{service,messages,groups,group_http,group_repository,model}.rs`: direct-write authorization and group additions |
| Realtime | `src/realtime/{model,delivery}.rs`, chat `models.ts`, `realtime.ts`, `sync.svelte.ts`: contacts refresh hints on the existing subscription |
| Contacts state | New `web/src/lib/features/contacts/{api,contacts.svelte,context.svelte}.ts`: canonical cache observers, operations and lifecycle |
| Contacts UI | New contacts `ContactsPage.svelte`, `AddFriendDialog.svelte`, `FriendPicker.svelte`, `contacts.css`; new `routes/app/contacts/+page.svelte` |
| Integration UI | `routes/app/+layout.svelte`, chat `ConversationActions.svelte`, `StartConversationDialog.svelte`, `GroupDetails.svelte`, `Composer.svelte`, and a new `AddGroupMembersDialog.svelte` |
| Contracts | `src/lib.rs`, `src/app.rs`, `src/bin/export_openapi.rs` and generated schemas |
| Verification | Rust test support, auth/friends/conversation/group tests; frontend unit tests, integration support and browser flows |

---

## Task 1: Open Registration

**Interfaces**

- `RegisterInput { username: String, display_name: Option<String>, password: String }`.
  Missing, empty or whitespace nickname uses the normalized username.
- Preserve `POST /api/v1/auth/register`, 201 response and issued session cookie.
- Preserve `auth::service::register(state, input) -> Result<AuthenticatedUser, AppError>`.
- Remove `create_invitation`, invitation repository operations and admin `Invite`.

- [x] Replace the invitation-consumption test in `tests/auth_test.rs` with
  `open_registration_creates_account_and_session`: no invitation field; assert
  201, authenticated `/auth/me`, and nickname fallback for omitted/blank values.
- [x] Add `duplicate_registration_is_atomic`: concurrent normalized equal
  usernames produce one 201 and one 409, exactly one account and one session.
  Retain Origin, password, cookie and rate-limit tests.
- [x] Run `cargo test --test auth_test`; confirm these new cases fail against
  the old invitation contract using the repository's test infrastructure.
- [x] Implement registration without the invitation lock/consumption; keep account
  and first-session insertion in one transaction. Remove the invitations table
  from `0001_identity.sql`, leaving sessions/outbox intact.
- [x] Remove registration invitation UI, prefill and admin command. Use “Create account”
  and “Register” copy; retain optional bootstrap with wording that points to public registration.
- [x] Update `tests/support/mod.rs`, `web/tests/integration/support.ts`,
  `web/e2e/fixtures.ts`, auth unit/browser tests and `web/e2e/deployment.spec.ts`
  to register without inserting invitation rows or requiring staging tokens.
  Search all remaining invitation references and preserve group invitations.
- [x] Regenerate contracts with `node scripts/generate-api.mjs`. Run auth tests,
  frontend auth unit tests and types; assert the invitation table is absent on a
  freshly migrated test database.

## Task 2: Friend Requests and Mutual Relationships

**Interfaces**

- `FriendshipView { user: UserView, created_at }`.
- `FriendRequestView { id: Uuid, sender: UserView, recipient: UserView,
  status: FriendRequestStatus, created_at, updated_at }`.
- `FriendRequestStatus`: `pending | accepted | declined | cancelled`.
- `FriendRequestInput { user_id: UserId }`;
  `FriendRequestActionInput { action: accept | decline | cancel }`.
- `GET /api/v1/friends` returns accepted friends with optional `q`, bounded page
  size 50 and an opaque keyset cursor.
- `GET /api/v1/friends/{user_id}` returns
  `RelationshipView { user: UserView, is_friend: bool, pending_request: Option<FriendRequestView> }`.
  This is the authoritative per-person UI lookup; absence from a loaded friend
  page is not evidence that a relationship is absent.
- `GET /api/v1/friend-requests` returns participant-owned requests with
  `direction=incoming|outgoing`, optional status and the same page/cursor contract.
  List responses contain `items` and `next_cursor`; request responses also include
  `pending_incoming_count` computed for the actor, independently of the page filter.
- `POST /api/v1/friend-requests` returns a request record.
- `PATCH /api/v1/friend-requests/{id}` returns the transitioned record.
- `DELETE /api/v1/friends/{user_id}` returns 204, including an already-absent pair.
- Public service operations:
  `request(state: &AppState, actor: Actor, peer: UserId)`,
  `respond(state: &AppState, actor: Actor, id: Uuid, action: FriendRequestAction)`,
  `remove(state: &AppState, actor: Actor, peer: UserId)`.
  The first two return `Result<FriendRequestView, AppError>`; remove returns unit.
- `require_friend(transaction: &mut Transaction<'_, Postgres>,
  actor: UserId, peer: UserId) -> Result<(), AppError>` locks the canonical pair
  and returns 403 when no accepted friendship exists.

- [x] Create `tests/friends_test.rs` covering recipient-only acceptance/decline,
  sender-only cancellation, self/unknown targets, duplicate requests, crossed
  requests remaining pending, accepted relationships and idempotent deletion.
  Assert request bodies reveal no passwords, session tokens or email fields.
- [x] Add stale-action and race cases: cancellation followed by a fresh request
  cannot be undone by accepting the old ID; repeated accept is idempotent while
  the friendship exists and never recreates a friendship removed later.
  Add pagination/search cases proving stable complete traversal.
- [x] Run `cargo test --test friends_test` and confirm the routes are absent.
- [x] Create the friends feature and schema. Store requests separately from
  friendships. Persist canonical `user_low < user_high` on both; unique friendship
  pair and partial unique pending-request pair prevent duplicates.
- [x] Serialize mutations using transaction-scoped advisory locks derived from
  canonical pair keys with a dedicated namespace. Sort multiple pairs before
  locking. Re-read the request status after locking; terminal stale actions
  return 409. Never treat an accepted historical request as current authorization.
- [x] Already-friends requests return 409; repeated pending or crossed requests
  return the existing pending record, preserving its original direction.
  Sending new requests is limited to 10 per authenticated user per minute;
  accepting/declining is not charged to this outgoing-request limit.
- [x] Wire routes and OpenAPI registration, regenerate schemas, run all friend
  tests and verify database uniqueness and actor isolation.

## Task 3: Contacts Invalidation Through the Existing Outbox

**Interfaces**

- Realtime payload: `{ kind: "contacts.updated" }`.
- Outbox payload: `{ type: "contacts.updated", user_id: UserId }`.
- `friends::repository::enqueue_contacts_changed(transaction, user_id)
  -> Result<(), sqlx::Error>` creates a uniquely keyed job in the same transaction.
- No contacts notification contains conversation sequence values or private profiles.

- [x] Extend `tests/outbox_test.rs` to assert request creation, acceptance,
  decline/cancellation and removal enqueue refresh jobs for both participants,
  and rolled-back mutations enqueue none.
- [x] Add delivery coverage for private recipient routing, retry after transport
  failure and harmless duplicate publication; verify old job kinds still work.
- [x] Implement the repository writer and `realtime::delivery` branch using
  `personal:#<user_id>`. Extend `EphemeralEvent` and generated schemas.
- [x] Extend frontend event validation and tests in
  `web/tests/unit/realtime-validation.test.ts` to accept the contacts hint while
  preserving rejection of invalid conversation sequence envelopes.
- [x] Run outbox/realtime tests. Confirm contacts hints do not enter conversation
  replay or acknowledgement logic.

## Task 4: Friend-Gated Direct Messaging and Atomic Group Additions

**Interfaces**

- Reuse `friends::service::require_friend` from Task 2.
- Keep existing group creation request fields and require every selected initial
  member to be the creator's friend.
- New `AddGroupMembersInput { membership_id: MembershipId, member_ids: Vec<UserId> }`.
- `POST /api/v1/conversations/{id}/members` delegates to
  `groups::add_members(state: &AppState, actor: Actor, id: ConversationId,
  expected: MembershipId, member_ids: &[UserId]) -> Result<Vec<GroupMemberView>, AppError>`.

- [x] Extend `tests/conversations_test.rs`: strangers cannot create direct chats;
  accepted friends can; removing then re-accepting reuses the existing direct ID.
  Old history remains readable after removal but new sends return 403.
- [x] Extend message tests for an identical committed-message retry after removal:
  it returns the original message; a new message/client ID is rejected.
  Race removal against sending and assert the outcome matches lock order.
- [x] Extend `tests/groups_test.rs`: all initial members must be friends; any
  active member may add their own friends; others cannot. Test duplicate/current
  members, invalid targets, capacity overflow, stale actor membership, concurrent
  removals/additions and rejoining with a new history boundary. Assert failures
  create no partial memberships/events.
- [x] Run the affected tests and observe failures before implementing policy.
- [x] Check friendship inside direct creation and new-message transactions.
  Preserve the committed-message idempotency branch before the new-send policy.
  Existing conversation operations acquire the conversation lock before pair
  locks; relationship mutations never acquire conversation locks. Review direct
  creation's uniqueness arbitration to avoid a reversed lock dependency.
- [x] Implement initial-member authorization and add-members under the existing
  group aggregate lock. Filter self/active members and deduplicate first; validate
  all remaining friendships/capacity before any membership write. Allocate each
  membership's join sequence from its durable event and append through existing
  event/outbox helpers in one commit.
- [x] Update Rust/browser integration setup helpers to explicitly establish
  friendships via requests and acceptance before scenarios that require them.
  Do not auto-friend everyone in a generic registration fixture.
- [x] Run conversation, message and group tests, including existing invitation,
  removal, role, history and media authorization cases.

## Task 5: Session-Owned Contacts Cache and Lifecycle

**Interfaces**

- `createContactsApi(client)` owns typed methods `friends`, `relationship`, `requests`,
  `request`, `respond` and `remove`; all reads accept an AbortSignal.
- `ContactsState` consumes the API, QueryClient and authenticated user ID;
  exposes `start()`, `refresh()`, `stop()`, canonical paginated friend/request
  query results, pending incoming count and mutation methods.
- `provideContacts()` / `useContacts()` expose the authenticated-tree owner.
- Extend chat coordinator options with optional
  `onContactsChanged: () => void` and `onReconnect: () => void` callbacks;
  the application layout composes them with contacts refresh.

- [x] Add `web/tests/unit/contacts.test.ts` for request mutations and invalidation,
  pagination, refresh coalescing, out-of-order reads and failed retries.
  Assert removing a friend updates permission consumers without copying records
  into another mutable server-record store.
- [x] Add cache/lifecycle tests: startup/reconnect recovers a missed hint;
  same-user profile refresh preserves draft state; stop/logout aborts requests
  and fences late responses; contacts hints leave conversation cursors unchanged.
- [x] Run the new tests and confirm the missing contacts owner causes failure.
- [x] Implement contacts query keys, observers, mutation invalidation and bounded
  reads. Obtain pending counts from a server count field on incoming pending
  request pages, not just the first page's item count. Derive composer eligibility
  from the per-person relationship query, never from a partial friend list.
- [x] Compose contacts in `routes/app/+layout.svelte` alongside the current chat
  owner. Route contacts hints/reconnect callbacks through that composition and
  refresh on the existing foreground reconciliation path. Keep one socket.
- [x] Run contact/cache/realtime unit tests and frontend types.

## Task 6: Contacts, Add Friend, Friend Picker and Group Member UI

**Interfaces**

- Contacts route: `/app/contacts`, label “Contacts”, pending-request badge.
- `AddFriendDialog` accepts bindable `open` and an optional preselected UserView.
- `FriendPicker` accepts `selected: UserView[]` as bindable state and
  `excludedIds: string[]`; it searches/pages accepted friends through contacts.
- `AddGroupMembersDialog` consumes the current conversation and membership ID.
- Plus menu: “Add friend”, “Start conversation”, “Join group”, preserving `align="start"`.

- [x] Add `web/e2e/friends.spec.ts`: two public signups, user search, request
  submission, recipient badge/acceptance, friend list and direct chat.
  Confirm sending a request alone does not create a conversation.
- [x] Add browser cases for decline/cancel, friendship deletion, history visibility,
  blocked new messages and re-acceptance. Cover crossed requests with explicit
  acceptance rather than a misleading second request button.
- [x] Update picker/group tests: select one friend for direct chat, several for
  group creation, then add another friend as an ordinary member. Verify immediate
  group discovery for the added member and retained invitation-link joining.
- [x] Run the affected browser cases to establish missing-feature failures.
- [x] Build the contacts list, incoming/outgoing request views and add-friend
  dialog using existing primitives. Show loading, empty, error and retry states,
  with per-operation disabled controls instead of dropping selections on failure.
- [x] Extract the reusable friend picker from the current username-search picker.
  Keep chip removal, debounce, IME, keyboard selection and stale-request guards.
  Global account search belongs only in Add friend.
- [x] Integrate group additions, sidebar/menu and direct composer permissions.
  An unresolved/erroring friendship read must not briefly enable sending; show a
  retry/loading state instead of incorrectly declaring the user a stranger.
  Preserve drafts across relationship refreshes and navigation.
- [x] Run browser tests at desktop and mobile widths, inspect screenshots, verify
  focus return to the plus trigger and dialog containment, and update menu-count
  assertions in `web/e2e/ui-controls.spec.ts` to the three approved actions.

## Task 7: Documentation, Fresh-Install Verification and Final Review

- [x] Update `docs/architecture.md`, `docs/operations/development.md`,
  `deployment.md`, `chat-experience.md`, `groups-and-media.md` and applicable
  acceptance docs. Distinguish removed registration invitations from retained
  group invitations. Document a fresh development database requirement without
  resetting unrelated Compose services or rewriting source/configuration.
- [x] Audit remaining registration invitation references across source, fixtures,
  scripts, schemas and active documentation. Historical design documents may
  describe past behavior but must not be presented as current onboarding.
- [x] Regenerate contracts, format source, inspect `git diff --check`, and verify
  architecture boundaries, comments, ownership and all new API error outcomes.
- [x] Run `node scripts/check.mjs --integration` serially with the repository's
  owned test infrastructure. Resolve failures and rerun affected gates.
- [x] Review final changes against the approved spec, particularly lock ordering,
  session teardown, direct-message policy, group history and API/UI consistency.
- [x] Report actual verification results, remaining limitations and uncommitted
  status. Do not publish, deploy or commit automatically.

## Execution

Recommended: native execution in this session, because the authorization, outbox
and frontend lifecycle tasks share contracts and are strongly ordered. Preserve
the existing working tree; do not start from a clean checkout that omits prior work.
Implementation starts after the plan review and execution-method selection.

## Delivery record

Implemented on the existing feature checkout without committing or deploying.
Policy regressions are consolidated in `tests/friends_test.rs`; related legacy
fixtures explicitly establish friendships. The execution ledger records observed
failures, review fixes and the final verification results.

The full integration command passed all non-browser phases and 44 of 46 browser
cases. Two test-harness issues were corrected (an ambiguous status locator and
sending before relationship resolution); all 14 cases in the affected files then
passed. An additional request-lifecycle browser case passed in the final four-case
friends run, completing 47 distinct browser scenarios across those runs.
