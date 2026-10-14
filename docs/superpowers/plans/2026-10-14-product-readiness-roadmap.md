# Product readiness roadmap

**Status:** Roadmap confirmed by the user. Implementation has not started. The user
selected verified email and email-based password recovery, and deferred iPhone/iPad
real-device push acceptance because suitable devices are currently unavailable.

**Goal:** Complete background notifications, account self-service, release verification,
and documentation consistency for the existing self-hosted small-team application.

**Architecture:** Retain the root Rust application, static SvelteKit client, independent
worker, PostgreSQL consistency model, Centrifugo transport and SeaweedFS media.
Account security remains in authentication; reactions remain in conversations.
Background notification delivery receives its own bounded durable workflow.

**Tech stack:** Existing locked Rust/Axum/SQLx/PostgreSQL and SvelteKit 3/Svelte 5
stack; Tokio SMTP delivery with `lettre`; browser Service Worker, Web App Manifest,
Push API and standard Web Push/VAPID.

**Design baseline:** [Product design](../specs/2026-10-12-chat-design.md),
[current chat experience](../specs/2026-10-14-chat-experience.md), and
[architecture ownership](../../architecture.md). This roadmap decomposes the next
work into independently reviewable designs and implementation plans.

## Scope and constraints

- Cover assessment items 1, 2, 3 and 5. Capacity testing, extended stability testing,
  and production monitoring/backup automation from item 4 remain a separate workstream.
- Item 1 includes PWA installation, background Web Push and message reactions.
  Account self-service includes changing a known password and recovering a forgotten one.
- Keep English engineering material and the existing Chinese product UI.
- Keep existing API paths, error contracts, registration fields and database semantics.
  Introduce additive endpoints and new immutable migrations where these features need them.
- Generate OpenAPI and TypeScript schemas from Rust; never edit generated files manually.
- Retain one root `compose.yaml`. Do not select or provision a production platform here.
- Commit, push, PR creation and deployment are later operator actions requiring the
  user's explicit instruction. Prepare concrete changes and validation evidence first.
- Do not cache authenticated API responses, messages, attachments, transfer grants,
  credentials or account recovery material in the PWA asset cache.
- Push transport success means provider acceptance. Notification timing and final
  presentation depend on browser, OS, permission and network conditions.
- At the user's request, iPhone/iPad real-device push acceptance is deferred and
  does not block development, automated verification or delivery of the remaining
  confirmed scope. Keep iOS push explicitly unverified until that device check runs.

## Verified starting point

The assessment in this conversation checked local commit `9f74897` with
`node scripts/check.mjs --integration`: 26 architecture checks, 90 Rust tests,
78 frontend unit tests, 7 SDK/recovery tests and 27 Chromium browser scenarios passed.
Formatting, Clippy, Svelte types, generated API and the static build also passed.
These counts describe that revision and must be refreshed after implementation.

The remote main revision observed during the assessment was `da49af3`. Its latest
main-branch CI run failed; local commits include a later CI fix and chat features.
No HTTPS staging origins were configured during the assessment. Writing this plan
does not establish hosted CI or staging acceptance for a new revision.

## Delivery sequence

| Milestone | Assessment item | Deliverable | Dependency | Effort |
| --- | --- | --- | --- | --- |
| A | 3 and 5 | Consistent baseline docs, current-revision CI and HTTPS staging baseline | Operator access and a disposable staging target for external verification | Small code scope; external setup can dominate |
| B | 2 | Self-service password change | Existing authentication/session revocation | Medium |
| C | 2 | Verified email, durable mail and email-based recovery | B's credential/session workflow; operator SMTP configuration | Large; split enrollment/mail and recovery into two deliverables |
| D | 1 | Installable PWA with safe asset/update behavior | Static build and HTTPS routing | Medium |
| E | 1 | Durable background push and device controls | B/C revocation lifecycle, D service worker, HTTPS target | Large |
| F | 1 | Message reactions with replay and multi-device convergence | Existing message revision/event model | Medium |
| G | 3 and 5 | Current-revision CI/staging evidence and final operator/user documentation | B through F | Medium plus real-device verification |

A establishes a baseline; G repeats release verification for the final changed
revision. A pending staging address does not prevent local B through F development,
but final delivery must retain that outstanding external acceptance explicitly.
Documentation is updated with every milestone, rather than deferred to G.

## Milestone A: release baseline and documentation correction

**Ownership:** `.github/workflows/ci.yml`, `scripts/check.mjs`,
`tests/infrastructure.mts`, `web/e2e/deployment.spec.ts`,
`docs/operations/{continuous-integration,deployment,groups-and-media}.md`, and `README.md`.

- [ ] Reconcile current operating guides: remove the obsolete production Compose/setup
  instructions in `groups-and-media.md`, and its claim that editing/search are future work.
  Keep historical acceptance records clearly labeled as historical evidence.
- [ ] Check that all three CI jobs retain isolated infrastructure, bounded diagnostics,
  safe artifact selection and owned-resource cleanup. Do not weaken failing tests.
- [ ] Prepare the exact candidate revision and local evidence. After an explicitly
  requested push/PR, require Backend, Frontend, and Contracts and integration to pass
  for that same SHA; do not substitute an older successful run.
- [ ] Configure a disposable HTTPS staging application and a separate media origin
  using the existing deployment guide. Supply fresh invitations through protected
  environment configuration, without putting secrets in this plan or logs.
- [ ] Run `pnpm -C web test:deployment` against that target and record the tested SHA,
  build media origin, command, result and browser. The command does not provision services.

**Acceptance:** Current-version CI evidence is linked; HTTPS deep links, cookies,
WebSocket routing, private endpoints, signed upload/download and CORS pass. Missing
operator access is recorded as an external dependency, never as successful verification.

## Milestone B: self-service password change

**Ownership:** `src/auth/{model,http,service,repository,password}.rs`,
`src/app.rs`, `src/bin/export_openapi.rs`, frontend auth/settings adapters,
`web/src/lib/features/settings/PasswordForm.svelte`, and the settings route.
Keep the CLI reset workflow available.

**Proposed behavior:** A signed-in user enters their current password, new password
and confirmation. The server verifies the current credential and session, changes
the password, revokes all existing sessions and persists disconnect jobs atomically.
After success, the current browser clears its authenticated lifetime and returns
to login. Explain that all devices will need to sign in again before submission.
Preserve the existing 6–128 Unicode-scalar password policy.

- [ ] Design the additive `POST /api/v1/auth/password` request with
  `current_password` and `new_password`; keep raw credentials out of public results.
- [ ] Reuse bounded password work and the existing user/session lock discipline.
  Revalidate the credential version and active session at the mutation boundary so
  a concurrent reset cannot allow an old credential to change a newer password.
- [ ] Keep password update, session revocation and durable disconnect jobs in one
  transaction. A database/outbox failure must leave the old credential intact.
- [ ] Add a settings form through the feature adapter and session owner. Retain
  shadcn controls, password-manager semantics, labels and Chinese copy.
- [ ] Cover incorrect credentials, Unicode bounds, concurrent login/reset,
  transaction rollback, repeated form submission, and other-device REST/socket rejection.

**Acceptance:** A real two-device browser test shows the old password failing,
the new password working, and both prior sessions becoming unusable through the
existing revocation/refresh mechanism. Existing registration/login/CLI behavior passes.

## Milestone C: account recovery

**Selected approach:** Verified email and email-based recovery. Recovery codes and
operator-issued recovery links are excluded from this batch. Keep username/password
registration and username login intact; email is enrolled from account settings.
Accounts without a verified email keep the existing operator CLI fallback.

**Ownership:** Authentication/security workflow and a named repository for email
challenges; a bounded SMTP adapter under `src/infrastructure/`; a separate durable
mail delivery loop; settings/auth adapters and forms;
`web/src/routes/{verify-email,forgot-password,reset-password}/+page.svelte`;
real authentication, mail and browser tests; account/deployment documentation.
Keep HTTP, application transactions, persistence and external transport distinct.

### C1: verified email enrollment and durable mail

**Proposed behavior:** One verified recovery address per account. Adding or changing
it requires the current password and confirmation through the new mailbox. The old
verified address remains active until confirmation succeeds, then receives a change
notice. Replacing an address invalidates outstanding recovery challenges. Email does
not become a second login identifier in this release.

- [ ] Add immutable persistence for verified address, pending verification challenges,
  credential/address generations and leased mail jobs. Enforce uniqueness of the
  canonical verified address in the database; pending requests do not reserve it forever.
- [ ] Parse addresses with a maintained library. Trim outer whitespace, lowercase the
  domain, preserve the local part and delivery spelling, and avoid provider-specific
  dot/plus rewriting. Apply the same documented comparison policy to enrollment,
  uniqueness, change and recovery.
- [ ] Issue 256-bit random, purpose-bound, single-use verification tokens with a
  30-minute lifetime. Store token digests in challenge rows; confirmation rechecks
  the initiating account, address generation, credential generation and current session.
- [ ] Configure the existing worker with an independent SMTP delivery loop and a
  reviewed pinned `lettre` Tokio/rustls adapter. Challenge and mail intent commit
  together; network calls do not run inside the transaction or the realtime delivery lane.
- [ ] Protect replayable mail payloads with authenticated encryption using a protected
  mail-queue key: the worker needs the link token, although challenge rows retain only
  its digest. Remove encrypted payload material after completion/expiry and document
  key backup/rotation so queued work survives a controlled restart.
- [ ] Use required STARTTLS or implicit TLS for production, bounded connection/send
  deadlines, capped retries and challenge-expiry checks. SMTP acceptance is distinct
  from arrival in a mailbox; report delivery failures without exposing tokens or credentials.
- [ ] Keep missing mail configuration and provider outages from disabling existing
  username authentication/text chat. Disable unavailable email controls with actionable
  operator guidance; retain queued work/retry when a configured provider is temporarily down.
- [ ] Construct links from a configured public application origin, never a request
  Host header. Carry the token in the URL fragment, exchange it only in a POST body,
  and use no-store/no-referrer behavior. Link visits and mail scanners do not consume tokens.
- [ ] Add a settings email panel with pending/verified state, resend and change actions.
  Define current-password verification, an explicit confirmation action and Chinese guidance.
- [ ] Cover competing email claims, stale pending changes, address spelling/comparison,
  mail timeouts/restart/retry, challenge rollback, token secrecy and scanner-style GETs.

**Acceptance:** An owned SMTP fixture captures a real verification message; following
its link and confirming promotes only the intended account's address. An expired,
used or stale challenge cannot bind an address. A real staging mailbox confirms
production TLS, link origin and delivery independently of the fixture.

### C2: email-based password recovery

**Proposed behavior:** The forgot-password page requests a link using an email address.
Only a currently verified address can receive it. Known, unknown and unverified
addresses receive the same public acknowledgement. A 256-bit random reset token lasts
15 minutes. Submitting the token and new password atomically consumes the challenge,
updates the credential, invalidates all other pending security challenges, revokes all
existing sessions and persists disconnect jobs. The user then signs in normally.

- [ ] Add rate-limited request/complete operations to the auth feature adapter and
  generated API. Use comparable request behavior for existing/nonexisting addresses;
  never lock an account because someone requested recovery.
- [ ] Tie reset challenges to the verified address and credential generations.
  Resending must not let an anonymous caller invalidate a still-usable link; bound
  issuance and queue growth without blocking normal authentication.
- [ ] Prepare bounded password work, then lock/revalidate the user and challenge in
  the existing user-first order. Invalid passwords and failed commits do not consume
  the token or partially revoke devices.
- [ ] Integrate challenge invalidation with milestone B, successful email changes
  and CLI password reset. Links issued before a credential/address change cannot
  reverse that change afterward.
- [ ] Add forgot/reset pages and a login entry point. Opening a link displays a form;
  only an explicit valid submission changes the password. Keep credentials/tokens
  out of persistent browser storage and notification content.
- [ ] Cover concurrent token use, expiry, older email ownership, reset/change races,
  transaction rollback, lost successful responses, enumeration behavior and old-device
  REST/socket rejection. Reuse C1's durable mail and provider-failure tests.

**Acceptance:** Two competing submissions cannot both use one reset token. A completed
reset makes old passwords, sessions and outstanding links unusable. The new password
works through normal login. The real mail/browser path passes on the staging build.

## Milestone D: PWA installation and updates

**Ownership:** `web/static/manifest.webmanifest`, product icons,
`web/src/service-worker/index.ts` with its own TypeScript configuration,
`web/vite.config.ts`, root document/layout and a cohesive PWA feature owner.
Use SvelteKit 3's current `$app/service-worker`, `$app/env` and `$app/manifest`
interfaces rather than copying legacy configuration.

- [ ] Define stable application identity, start URL, scope, standalone display,
  theme/background colors, standard icons, maskable icons and Apple touch icon.
- [ ] Cache only an explicit allowlist of immutable build assets, public icons and
  a generic offline page. API, media, WebSocket and mutation requests bypass this cache.
- [ ] Keep service-worker asset network calls in shared transport under `api-client`;
  preserve the frontend HTTP boundary instead of exempting all worker fetch calls.
- [ ] Add `pnpm -C web exec tsc --project src/service-worker/tsconfig.json --noEmit`
  to the frontend check scope so the separately typed worker cannot bypass type checks.
- [ ] Keep navigation online-first; an offline fallback explains the unavailable
  connection without displaying a previous account's message history.
- [ ] Present an update notice that waits for an explicit user action. Avoid forced
  activation/reload while a composer draft, upload or submission is in progress.
- [ ] Test manifest routes, service-worker registration, asset allowlisting, old-cache
  cleanup, deployment update behavior and account switching with existing caches.

**Acceptance:** The application installs and reopens in supported desktop/mobile
environments. A deployment update preserves ongoing work until the user chooses
to reload. Cache inspection contains no authenticated records or signed transfer URLs.

## Milestone E: durable background Web Push

**Ownership:** A cohesive Rust notifications feature with HTTP/service/repository
boundaries; a bounded Web Push adapter under `src/infrastructure/`; worker composition;
authenticated frontend notification API/state owner; the PWA service-worker entry.
Preserve the existing chat notification policy and durable replay owner.

**Architecture choice:** Standard Web Push with stable operator-managed VAPID keys.
The Rust adapter uses a reviewed, pinned library for standard signing/encryption;
verify its Windows/Linux and Tokio compatibility before adopting it. Do not handwrite
the cryptography or require a separate notification platform as an initial dependency.
Browser-vendor push services remain external delivery dependencies.

- [ ] Add authenticated subscription registration/removal and device controls.
  Bind subscriptions to the actual session/user; handle endpoint replacement,
  account switching, logout, password changes and expired/revoked sessions.
- [ ] Validate submitted endpoints/keys, limit registrations and reject internal
  destinations or unsafe redirects. Treat endpoints and auth keys as private capabilities.
- [ ] Insert push intent in the committed message transaction, then deliver through
  a separate leased queue/loop. Push timeouts or retries must not delay message
  commits, realtime fanout, media work or session disconnects.
- [ ] Recheck current session, membership episode, message state, mute preference
  and read state before dispatch. Restrict this release to new incoming messages;
  edits, withdrawals, reactions and replay backlog do not generate new-message push.
- [ ] Define a short dispatch delay and expiring per-device foreground/focus lease
  to suppress duplicate in-page/background alerts where possible. Lease expiry
  restores eligibility; stale foreground state must not suppress delivery indefinitely.
- [ ] Bound retries by an intent deadline; handle provider throttling/transient
  failure separately from expired subscriptions. Remove invalid subscriptions on
  definitive provider responses. Preserve message/event durability independently.
- [ ] Use generic notification text without message bodies, sender names or group
  titles. Use validated same-origin click routes; clicking authenticates and reads
  current authorized state. No notification advances a replay or read cursor.
- [ ] Display a user-visible notification for accepted push events; do not depend on
  silent background pushes. Deduplicate with stable event tags and document the
  race where already accepted provider messages can arrive after revocation.
- [ ] Request permission only from an explicit user action. Give unsupported,
  denied and installation-required environments usable in-page feedback.
- [ ] Test actual payload encryption with test subscription keys, owned queue leases,
  provider error handling, retries, logout/revocation, group removal/rejoining,
  mute/read suppression and concurrent foreground transitions.

**Automated acceptance:** Verify subscription/session ownership, permission-state
fallbacks, asset-cache privacy, push payload encryption, queue/retry/expiry behavior,
mute/read filtering and authorized notification-click routing through code and
browser tests. Keep automated provider acceptance distinct from OS presentation.

**Real-device acceptance:** On available HTTPS desktop/mobile environments, confirm
that a notification arrives with the chat page closed/backgrounded and opens the
correct authorized conversation. Record provider/browser/OS versions and whether
the browser remained running. iPhone/iPad home-screen installation and OS push
presentation are deferred by the user; do not claim verified iOS support from browser
automation or a local provider fixture. This deferred check is not a blocker for
the other confirmed work.

## Milestone F: message reactions

**Ownership:** `src/conversations/` workflow/repository/model/replay projection,
generated contract, chat API, message-record merging, message actions and bubbles.
Use the existing transaction, membership-episode and exact-sequence model.

**Proposed behavior:** Six fixed reactions: like, love, laugh, surprise, sad and
celebrate. Each user selects at most one reaction per message; they can switch or
remove it. Show counts and the current user's selection. Reactions do not change
message creation order, the original send identity or unread message counts.

- [ ] Add immutable reaction persistence and additive set/remove operations scoped
  to a message and current membership episode. Repeating a desired state is idempotent.
- [ ] Commit a reaction mutation, revision, durable event and outbox together.
  Keep event sequence separate from message creation order and read semantics.
- [ ] Project reaction summaries from current authorized state across snapshots,
  history, search/context and replay. Compute the current user's selection for
  the REST reader rather than storing actor-specific flags in shared event payloads.
- [ ] Reject reactions to withdrawn or inaccessible messages and hide withdrawn
  reaction presentation. Fence old responses so they cannot resurrect stale counts.
- [ ] Add keyboard/mobile-accessible controls and compact counts using existing UI
  primitives. Keep the feature's network/state ownership outside components.
- [ ] Cover concurrent set/switch/remove, unauthorized and rejoined members,
  withdrawal races, offline replay, stale HTTP replies and two-device convergence.

**Acceptance:** Multiple real browsers converge on reaction counts and their own
selection after reconnect/reload. Reaction changes never generate new-message
notifications or inflate unread counts.

## Milestone G: integrated acceptance and final documentation

- [ ] Regenerate the contract and review migration/old-payload compatibility.
- [ ] Run `node scripts/check.mjs --integration` on the integrated revision, retaining
  actual command output. Repeat targeted checks for subsequent fixes; repeat unrelated
  stages only if the changed code invalidates their evidence.
- [ ] Extend HTTPS staging scenarios for account security, PWA assets and reaction
  convergence. Execute push acceptance on available real environments separately;
  list iPhone/iPad real-device push acceptance as deferred rather than passed.
- [ ] After an explicitly requested publish action, verify all three hosted jobs
  for the final SHA and record the staging build's matching version.
- [ ] Update `README.md`, architecture, account/recovery, notification/PWA, deployment
  configuration, migration/upgrade guidance and acceptance records in the same change.
  Document SMTP host/port, required TLS mode, protected credentials, sender identity,
  public application origin, mail-queue key and provider sender-domain verification.
- [ ] Review old operations guides for conflicting claims and broken commands.
  Retain historical records with clear dates/revisions instead of rewriting history.

**Completion evidence:** Exact source/build revision, local check results, hosted
job links, staging results, real-device push observations, and any remaining
provider/browser limitations. Do not mark a milestone done using configuration or
test definitions alone. The user's iPhone/iPad deferral remains an explicit pending
platform check and does not prevent completion of the other confirmed deliverables.

## Design and execution handoff

The [detailed design package](../specs/2026-10-14-product-readiness-design.md) now
contains written account security/recovery, PWA/background notification and reaction
designs approved by the user on 2026-10-14. Each receives a focused implementation plan. Keep release
verification and documentation attached to those deliverables. Designs must settle
wire contracts, migration names, concurrency rules, device lifecycle and exact
acceptance cases before their corresponding product code changes.

The recovery method is settled: verified email and email-based password recovery.
Staging addresses/invitations, operator SMTP and sender configuration, mail-queue key,
VAPID contact/key ownership and real test devices are execution inputs. They can be
supplied when their milestone is executed.

## Official references checked during planning

- [SvelteKit service workers](https://svelte.dev/docs/kit/service-workers): build,
  registration and the separate service-worker TypeScript context.
- [Push API](https://developer.mozilla.org/en-US/docs/Web/API/Push_API): browser
  subscriptions, service-worker delivery and private subscription capabilities.
- [Apple Web Push](https://webkit.org/blog/13878/): iOS/iPadOS home-screen application
  enrollment and permission through direct user interaction.
- [Web Push crate documentation](https://docs.rs/web-push/latest/web_push/): candidate
  Rust signing/encryption/transport APIs; compatibility still needs a scoped probe.
- [Email validation and verification](https://cheatsheetseries.owasp.org/cheatsheets/Email_Validation_and_Verification_Cheat_Sheet.html):
  consistent address comparison, ownership verification and address changes.
- [Password recovery guidance](https://cheatsheetseries.owasp.org/cheatsheets/Forgot_Password_Cheat_Sheet.html):
  single-use reset links, anti-enumeration behavior and explicit password submission.
- [Async SMTP transport](https://docs.rs/lettre/latest/lettre/transport/smtp/struct.AsyncSmtpTransport.html):
  Tokio integration and required STARTTLS/implicit TLS.
