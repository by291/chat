# Account Security Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans after execution-method selection. Steps use checkbox syntax.

**Goal:** Add self-service password change, verified email and email recovery with
atomic credential/session/challenge behavior and independent durable mail.

**Architecture:** Authentication owns security operations and transactions; named
repositories own SQL. SMTP/encrypted payloads are concrete infrastructure adapters.
The authenticated frontend lifetime owns identity teardown and fenced form results.

**Tech Stack:** Existing locked stack, `lettre =0.11.23`, `aes-gcm =0.11.1`, existing
secure RNG/base64/SHA256, real PostgreSQL and owned SMTP/browser fixtures.

**Spec:** [Approved account design](../specs/2026-10-14-account-security-design.md).

**Local status (2026-10-15):** Implemented and verified by the final combined
`node scripts/check.mjs --integration` gate (exit 0). See the
[acceptance record](../../operations/product-readiness-acceptance.md) for source
identity, counts, review follow-up and external checks that remain pending.

## Global Constraints

- Preserve username registration/login, existing CLI commands and 6–128 Unicode-scalar passwords.
- Keep all current APIs and Chinese UI; add only the approved account endpoints/DTOs.
- Tokens have 256 random bits; verification lasts 30 minutes and reset lasts 15 minutes.
- Required production TLS, 10-second mail deadline, 30-second leases, retry cap 60 seconds.
- Use the [owned Rust verification recipe](2026-10-14-product-readiness-execution.md#targeted-verification-commands); never use the development database.
- Regenerate schemas; keep migrations immutable; do not commit/push without separate instruction.
- Mail configuration is optional and disabled by default; provider failures cannot stop text/auth/realtime/media.

## Review Focus

1. A session may be revoked while password hashing runs: Task 1 tests transaction-time revalidation.
2. A new identity may appear before an old form completes: Task 2 tests authenticated-lifetime fencing.
3. A scanner may visit a token link before its owner: Tasks 4/5 test harmless GET and explicit POST.
4. SMTP may accept before a worker crash: Task 3 tests repeated mail with one valid challenge.
5. A mail token may target an older email/password version: Tasks 4/5 test stale generation rejection.

### Task 1: password change and version boundary

**Files:** Create `migrations/0012_account_security_versions.sql`,
`src/auth/security.rs`, `src/auth/security_repository.rs`, `tests/account_security_test.rs`.
Modify `src/auth/{mod,model,http,service,repository}.rs`, `src/app.rs`,
`src/bin/export_openapi.rs`, and explicit type-facade exports as needed.

**Interfaces:** Consume `PasswordService::{verify,hash}`, `Actor`, existing
session/outbox functions and caller-owned transactions. Produce
`security::change_password(state: &AppState, actor: Actor, input: PasswordChangeInput) -> Result<(), AppError>`
and `PasswordChangeInput { current_password: String, new_password: String }`.
The repository owns `SecurityIdentity { user_id, password_hash, credential_version, email_version }`,
with ID/hash/bigint types `UserId`/`String`/`i64`.
`load_identity(pool: &PgPool, user_id: UserId)` and
`lock_identity(tx: &mut Transaction<'_, Postgres>, user_id: UserId)` return
`Result<Option<SecurityIdentity>, sqlx::Error>`; the latter locks exclusively.
`require_active_session(tx: &mut Transaction<'_, Postgres>, actor: Actor)` returns
`Result<bool, sqlx::Error>` under a session row lock. Repository functions never commit.

- [x] Write HTTP regressions named `wrong_current_password_does_not_revoke_a_valid_session`, `password_change_revokes_all_devices_and_preserves_atomicity`, and `revoked_session_cannot_finish_password_change`. Arrange through existing `support::spawn_test_app` and `register_user`; assertions include:

```rust
// Assertion fragment after the wrong-password POST and owned-session GET.
assert_eq!(response.status(), 400);
assert_eq!(error_body["code"], "invalid_current_password");
assert_eq!(current_identity.status(), 200);
```

- [x] Format test code and run Rust target `account_security_test`. Expected RED: the new route returns 404 instead of the approved response; existing setup compiles and uses only isolated data.
- [x] Add nonnegative credential/email versions (default 0); implement prepare/hash, user lock, matching hash/version and active-session revalidation. Update/revoke/outbox commit together and increment the credential version.
- [x] Expose `POST /auth/password` with user rate 5/minute and stable errors. After success, revoke old sessions without a deletion cookie, preserving a newer shared-browser login; the final review regression supersedes the original cookie-clearing requirement.
- [x] Update CLI reset to increment the same credential version under the user lock. Do not let old-password login mint a post-change session; preserve shared/exclusive lock ordering.
- [x] Force outbox insertion failure and a deterministic preparation/revocation race in real database tests. Assert unchanged hash/version/sessions on rollback, old-password rejection/new-password acceptance on commit, and exact all-device revocation.
- [x] Run Rust target `account_security_test` and existing `auth_test`/`admin_test` separately through the owned recipe. Expected GREEN: all assertions pass. Generate API and run `api:check`.

### Task 2: password form and authenticated teardown

**Files:** Modify `web/src/lib/features/auth/{api,session.svelte}.ts`,
`web/src/lib/features/settings/api.ts`, `web/src/routes/app/settings/+page.svelte`.
Create `web/src/lib/features/settings/PasswordForm.svelte`,
`web/tests/unit/account-security.test.ts`, `web/e2e/account-security.spec.ts`.

**Interfaces:** Consume Task 1's generated DTO. Produce
`createAuthApi(...).changePassword(input: PasswordChangeInput): Promise<void>` and
`SessionState.changePassword(input: PasswordChangeInput): Promise<void>`.
Capture the session generation before the request; only the current lifetime resets
on a successful acknowledgement. The component uses the auth owner, never raw HTTP.

- [x] Write state regressions for inline 400 errors, successful reset callbacks and a late successful acknowledgement after identity change. Assertion fragment after arranging a controlled API and session:

```ts
expect(session.user).toBeUndefined();
expect(resetCalls).toBe(1);
expect(currentIdentityAfterLateOldReply).toEqual(newIdentity);
```

- [x] Run `pnpm -C web exec vitest run tests/unit/account-security.test.ts`. Expected RED: the change-password operation/state lifecycle is absent.
- [x] Implement typed API/state boundaries and a Chinese form with current/new/confirmation fields, password-manager autocomplete, explicit submit type, pending guard and a visible all-device sign-out explanation.
- [x] Clear password fields after success/teardown; abort/fence obsolete form work. Keep draft/upload/cache/socket teardown in existing session/chat owners.
- [x] Add a real two-device browser scenario: submit an incorrect current password without losing login, then change successfully, reject old REST/socket sessions, and log in using the new password.
- [x] Run the unit target, `pnpm -C web check`, and `pnpm -C web exec playwright test e2e/account-security.spec.ts`. Expected GREEN with real authentication/realtime infrastructure.

### Task 3: mail persistence, encryption, adapter and independent worker

**Files:** Create `migrations/0013_account_email_and_mail.sql`,
`src/auth/mail_repository.rs`, `src/auth/mail_worker.rs`, `src/infrastructure/mail.rs`,
`tests/support/mail.rs`, `tests/account_mail_test.rs`, `tests/account_mail_queue_test.rs` and
`web/tests/integration/mail-support.ts`.
Modify `Cargo.toml`/lock, `src/auth/mod.rs`, `src/infrastructure/{mod,config}.rs`,
`src/app.rs`, `src/bin/worker.rs`, `scripts/check.mjs`, `.env.example` and fixture configuration.

**Interfaces:** Produce `MailConfig`, `MailPurpose` with stored keys
`email_verification`, `password_reset`, `email_changed`,
`MailPayload { purpose: MailPurpose, recipient: String, link: String }`,
`MailDelivery`, and typed `LeasedMailJob`. Public adapter boundaries are
`MailDelivery::new(config: &MailConfig) -> Result<Self, AppError>`,
`seal(job_id: Uuid, purpose: MailPurpose, payload: &MailPayload) -> Result<Vec<u8>, AppError>`,
`open(job_id: Uuid, purpose: MailPurpose, encrypted: &[u8]) -> Result<MailPayload, AppError>`, and
`send(payload: &MailPayload) -> Result<MailOutcome, AppError>`.
`MailOutcome` distinguishes accepted, transient and permanent outcomes without raw
SMTP error text. Produce `mail_worker::run_worker(state, cancel) -> Result<(), AppError>`.
Email-change notices link to account settings and expire after 24 hours; token-bearing
mail jobs inherit the challenge's 30/15-minute deadline.

- [x] Add adapter/queue regressions for tampered ciphertext/AAD, fresh nonce, stale lease completion, deadline expiry, transient retry and post-send crash. Assertion fragments after arranging the job/cipher/fixture:

```rust
assert_ne!(first_ciphertext, second_ciphertext);
assert!(delivery.open(other_job_id, purpose, &first_ciphertext).is_err());
assert_eq!(accepted_messages[0].link, accepted_messages[1].link);
assert_eq!(live_challenge_count, 1);
```

- [x] Run Rust target `account_mail_test` and library `mail` tests using the owned recipe. Expected RED: mail adapter/queue contracts do not exist; do not fabricate SMTP success.
- [x] Add the three tables from the spec with constraints, indexes and encrypted mail payloads. Pin `lettre =0.11.23` and `aes-gcm =0.11.1`; select Tokio/rustls builder/pool/SMTP features and one explicit compatible crypto provider, then compile under Rust 1.99 before further work.
- [x] Construct full AppState with existing reqwest/S3 clients and enabled mail in the fixture, catching Rustls-provider/runtime conflicts. Required TLS verifies certificates; plaintext fixture transport is available only through a scoped test adapter, never a production bypass setting.
- [x] Validate the exact configured HTTPS application origin, TLS mode, sender and 32-byte queue key at enabled startup. Reject invalid enabled configuration with redacted diagnostics; construct links from that origin rather than request Host headers. Add tests for disabled/absent config and both required TLS modes, including certificate rejection and no downgrade.
- [x] Gate fixture transports with the existing `test-failpoints` feature and update the root Cargo test phase to `--all-features -- --include-ignored`. Also run `cargo build --locked --bin api --bin worker --bin admin` with default features; production configuration must have no transport-bypass switch.
- [x] Implement AES-256-GCM with random 12-byte nonce, 32-byte configured key, and job ID/purpose as authenticated data. Remove encrypted payloads on terminal outcomes and prevent Debug/log exposure.
- [x] Implement caller-composed mail intent persistence, 30-second leased claim/CAS, total send deadline 10 seconds and capped retry 60 seconds until expiry. Keep all network work outside SQL locks and all provider failures inside the independent loop.
- [x] Extend worker composition and fixture cleanup with owned SMTP listeners. Test cancellation during silent SMTP while existing realtime delivery still succeeds; update optional configuration examples without real secrets.
- [x] Provide a Rust-owned SMTP capture listener in `tests/support/mail.rs` and export `TestSmtpServer` in `web/tests/integration/mail-support.ts` with `start(): Promise<void>`, `messages(): CapturedMail[]`, `stop(): Promise<void>` and private fixture configuration. `CapturedMail` contains recipient, subject and decoded text/link. Extend `TestRuntime.start` options to accept that fixture's configuration so browser targets use actual captured messages. Neither listener sends onward to a real mailbox; their owners close sockets and remove only their own fixture resources.
- [x] Run adapter target `account_mail_test`, database target `account_mail_queue_test`, native `mail-transport.test.ts` and `node scripts/check.mjs --scope backend`. Expected GREEN: real captured delivery, exact cleanup/lease behavior, no unrelated worker shutdown and no unbounded external call.

### Task 4: verified email enrollment and settings

**Files:** Extend `src/auth/{model,http,security,security_repository}.rs`,
`src/app.rs`, `src/bin/export_openapi.rs`; create `tests/account_email_test.rs`.
Create settings `EmailForm.svelte` and auth `EmailVerification.svelte`;
modify auth adapter, settings route and `AuthForm.svelte`;
create `web/src/routes/verify-email/+page.svelte`,
`web/e2e/account-email.spec.ts`, and unit `account-email.test.ts`.

**Interfaces:** Produce `EmailStatus` and enrollment/verification DTOs from the spec.
Application functions are `email_status(state, actor) -> Result<EmailStatus, AppError>`,
`enroll_email(state, actor, input: EmailEnrollmentInput) -> Result<(), AppError>`, and
`verify_email(state, actor, token: &str) -> Result<EmailStatus, AppError>`.
Add optional `onAuthenticated` callback to `AuthForm`; ordinary login/register routing
retains current behavior, while inline verification sign-in stays on the fragment route.

- [x] Write real-mail/HTTP tests for disabled mail, old-address retention, duplicate pending claims, successful confirmation, wrong initiating actor, case normalization, scanner GET and changed-version tokens. Assertion fragment after capturing a link:

```rust
assert_eq!(verified_email_before_confirmation, original_email);
assert_eq!(scanner_get.status(), 200);
assert!(!challenge_consumed_after_get);
assert_eq!(wrong_actor_confirmation.status(), 403);
assert_eq!(verified_email_after_confirmation, new_email);
```

- [x] Run Rust target `account_email_test`. Expected RED: status/enrollment/confirmation contracts are absent.
- [x] Implement canonical parsing, current-password/session fences, 30-minute token digests and challenge+encrypted-mail atomicity. Unique verified ownership is enforced at confirmation; pending addresses do not reserve ownership.
- [x] Apply the five-attempt current-password limit to enrollment as well as password change. Keep normalized target addresses hashed in rate-limit keys and retain fail-closed dependency behavior.
- [x] Consume verification under user/challenge locks, increment email version, invalidate pending security challenges and enqueue the old-address notice atomically. Add 3/user and target/15-minute enrollment limits.
- [x] Regenerate API, implement typed settings and verification owners, and retain fragment tokens only in the route lifetime. Inline sign-in must not encode a token into a logged `next` query or persist it in storage.
- [x] Add no-store security responses and no-referrer token pages without analytics/third-party assets. Clear the fragment after completion; test headers, browser storage and response/log token secrecy.
- [x] Unit/browser regressions cover logout/account change during verification, resend/expiry, keyboard forms and the captured link's full same-account sign-in/explicit-confirm path. Task-specific unit assertions inspect history URLs and storage for token absence outside the allowed fragment.
- [x] Run Rust/units/browser targets, frontend types and contract check. Expected GREEN: the real owned mailbox and browser verify only the intended owner and never consume a link on GET.

### Task 5: forgotten password and atomic recovery

**Files:** Extend auth security/model/repositories/HTTP, route/export wiring,
auth adapter and unit tests. Create `tests/account_recovery_test.rs`,
auth `PasswordRecovery.svelte` and `PasswordReset.svelte`,
`web/src/routes/{forgot-password,reset-password}/+page.svelte`,
and `web/e2e/account-recovery.spec.ts`; add the login entry point.

**Interfaces:** Produce
`request_password_recovery(state, email: &str, client_ip: IpAddr) -> Result<(), AppError>`
and `reset_password_from_token(state, input: PasswordResetInput) -> Result<(), AppError>`.
Produce `invalidate_security_challenges(tx, user_id) -> Result<(), AppError>` and reuse
it in Task 1/CLI transactions. Request returns empty 202; reset returns 204 without login.

- [x] Write known/unknown/unverified/throttled request, three-live-link cap, harmless GET, concurrent reset, rollback and stale-email/credential tests. Assertion fragment after two competing submissions and a forced failed commit:

```rust
assert_eq!(successful_resets, 1);
assert_eq!(active_sessions_after_reset, 0);
assert!(!challenge_consumed_after_failed_commit);
assert_eq!(unknown_request.status(), known_request.status());
assert_eq!(known_request.status(), 202);
```

- [x] Run Rust target `account_recovery_test`. Expected RED: recovery/reset endpoints return 404.
- [x] Implement 15-minute, 256-bit reset challenges and limits of 10/IP and 3/target/15 minutes; target throttling stays generic. Enforce three active links without anonymous resend invalidating usable links.
- [x] Recheck verified address and both versions under user/challenge locks. Change hash/version, consume/invalidate tokens, revoke sessions and enqueue disconnects atomically. Extend self-service and CLI password changes to invalidate all pending challenges.
- [x] Regenerate API and implement fragment reset forms with no automatic login, no persisted token/password and localized generic request acknowledgement.
- [x] Exercise a captured mail link in real browsers, including scanners, another active device and old credentials. Cover lost successful responses by new-password login rather than repeated password mutation.
- [x] Run Rust/unit/browser targets plus existing auth/admin tests. Expected GREEN with no account enumeration in response shape or partial durable changes.

### Task 6: complete account integration and operations

**Files:** Update architecture, development/deployment and new
`docs/operations/account-security.md`; extend fixture backup/recovery cases and
the product-readiness acceptance record through the coordinator plan.

**Interfaces:** All approved account DTOs/paths plus the mail worker's optional config
and lifecycle. Consumption by push is the existing session revocation database boundary.

- [x] Add a populated-schema upgrade regression from migration 11: existing passwords/sessions/accounts remain valid, versions start at 0, and no account is silently email-enrolled.
- [x] Extend the real backup drill with a pending encrypted verification/reset mail job; use the matching protected key in the restored fixture and assert old/revoked session rules still hold.
- [x] Run `pnpm -C web api:check` and `node scripts/check.mjs --integration` after account tasks are integrated. Expected GREEN with actual SMTP/database/browser checks included.
- [x] Document enrollment, address case policy, re-login after password change, expired/resend/lost-response behavior, SMTP acceptance versus mailbox arrival, key backup/drain-before-rotation, and CLI fallback.
- [x] Review the task diff and record actual results. Staging mailbox delivery remains pending until protected operator SMTP inputs and a consenting mailbox exist.

Library references checked for planning: [lettre](https://docs.rs/lettre/latest/lettre/)
and [AES-GCM](https://docs.rs/aes-gcm/latest/aes_gcm/). Run compatibility constructors
and real TLS fixtures before treating dependency selection as verified integration.
