# Account security and email recovery design

**Status:** Written design approved by the user on 2026-10-14. The roadmap and email
recovery choice are confirmed; this document settles contracts before implementation.

**Goal:** Let existing username accounts change their password, verify a recovery
email and reset a forgotten password without losing transaction/session guarantees.

## Scope and ownership

Keep username registration/login, the 6–128 Unicode-scalar password policy, existing
CLI commands, Chinese UI and all existing API contracts. Email enrollment is optional;
accounts without verified email retain the operator reset fallback. Email does not
become a login identifier. Recovery codes and a web administrator console are outside
this batch.

Authentication owns the policy in `src/auth/security.rs`, DTOs in `auth/model.rs`
and SQL/locking in `auth/security_repository.rs`. Existing `auth/http.rs` remains a
transport adapter. Durable mail uses cohesive `auth/mail_worker.rs` and
`auth/mail_repository.rs`; SMTP and encrypted mail payloads belong to
`infrastructure/mail.rs`. The existing worker composes an independent mail loop.
No generic repository/service traits or additional crates are introduced.

Frontend auth adapters own HTTP; the session owner clears authenticated state.
Settings adds password/email forms through existing shadcn primitives. Public routes
are `/verify-email`, `/forgot-password` and `/reset-password`; token-bearing routes
remain browser-only and do not require entering the chat tree.

## Public contracts

All paths below are under `/api/v1`. Errors retain `ApiErrorBody`, Origin protection
and request correlation. Rust generates the OpenAPI/TypeScript contract.

| Operation | Input | Result |
| --- | --- | --- |
| `POST /auth/password` | `PasswordChangeInput { current_password, new_password }` | `204`, clear current cookie after atomic revocation |
| `GET /auth/email` | Authenticated actor | `EmailStatus { enabled, verified_email, pending_email, verification_expires_at }` |
| `POST /auth/email` | `EmailEnrollmentInput { email, current_password }` | `202`, verification intent committed |
| `POST /auth/email/verify` | Authenticated actor and `EmailVerificationInput { token }` | `200 EmailStatus` |
| `POST /auth/password-recovery` | `PasswordRecoveryInput { email }` | `202` with an empty body for known/unknown/unverified addresses |
| `POST /auth/password-reset` | `PasswordResetInput { token, new_password }` | `204`, no automatic login |

Status emails/timestamps are nullable; timestamps use existing RFC3339 representation.
Only the owner receives email status; peer/profile DTOs acquire no email fields.
An incorrect current password returns `400 invalid_current_password`, not a session
`401`: the frontend's existing global 401 handler would otherwise log out a valid
session. Invalid, expired, consumed or invalidated challenge tokens share
`400 invalid_security_token`. Unauthenticated protected operations retain `401`.
An address uniqueness conflict after mailbox confirmation uses `409 conflict`.
Disabled email operations use `503 email_unavailable`; password change remains usable.

## Data model and lifecycle

Add immutable migrations in execution order; do not reserve or rewrite earlier files.
The first password-change migration adds `users.credential_version bigint NOT NULL DEFAULT 0` and
`users.email_version bigint NOT NULL DEFAULT 0` with nonnegative constraints.
The following email milestone adds challenge/mail tables and extends password/CLI
transactions with their invalidation. The version fence works before email enrollment
exists, allowing password change to remain an independently testable deliverable.

- `account_emails`: one row per user, original delivery address, canonical address,
  verification time; unique canonical verified address.
- `account_security_challenges`: UUID, user, purpose (`email_verification` or
  `password_reset`), unique token digest, optional candidate address, captured
  credential/email versions, creation/expiry/consumption/invalidation timestamps.
- `account_mail_jobs`: UUID, purpose, optional challenge ID, encrypted payload,
  expiry, retry schedule, attempt count, lease token/deadline and terminal status.

Use a library email parser. Trim outer whitespace, lowercase the domain and preserve
the local part in comparisons; preserve the original address for delivery/display.
Do not rewrite provider dot/plus aliases. Apply this rule consistently to uniqueness,
enrollment and recovery, and test mixed-case domains and distinct local-part spelling.

Tokens contain 256 random bits from the existing secure RNG; store SHA256 digests in
challenge rows. Verification expires after 30 minutes; reset expires after 15 minutes.
Token purpose and captured versions are checked during consumption. Challenge reads
never expose raw tokens. Security responses use no-store behavior.

Changing a password, recovering it or using the CLI reset increments the credential
version and invalidates all outstanding security challenges in the same transaction.
A verified address stays verified after password change. Confirming an email replaces
the verified address, increments email version and invalidates outstanding challenges.
It preserves sessions because the password did not change.

## Password change and concurrency

Prepare password verification/hashing through the bounded password service outside
the final mutation transaction. Capture the old credential hash/version; under an
exclusive user lock, recheck that hash/version and that the actor session is still
active before changing credentials. A mismatch rejects stale work.

Lock order is user, security challenge where relevant, sessions, then durable jobs.
This matches existing login's user lock before session creation. Caller-owned
transactions commit only in application operations; repositories never commit.

The credential update, version increment, challenge invalidation, revocation of all
existing sessions and existing realtime disconnect jobs commit together. Any failure
rolls back the whole change. The HTTP adapter then clears the current cookie; the
session owner resets cache/socket/upload/notification lifetimes and navigates to login.
Other devices follow the existing immediate REST rejection and disconnect/refresh
fallback, without inventing a zero-latency distributed revocation guarantee.

## Email enrollment and confirmation

Enrollment verifies the current password and session using the same stale-work fence.
It replaces that actor's pending verification attempt and commits challenge plus mail
intent atomically. The currently verified address stays valid until confirmation.
Two users may have pending requests for the same mailbox; only verified ownership
is unique, so a pending request cannot permanently reserve another person's address.

Confirmation requires an authenticated session for the challenge's initiating user,
an explicit POST, matching versions and a live token. Recheck unique ownership under
the database constraint. On success, consume the challenge, replace the verified
address and enqueue a change notice to the previous address if it differed. Neither
the notice nor the response names another account.

Verification links use `/verify-email#token=<opaque-token>`. The route retains the
fragment token in memory and can render the existing login form inline when needed;
authentication succeeds without moving the token into a query-string `next` parameter.
Only explicit confirmation submits the token in a POST body. Opening the link or
a mailbox scanner's GET does not consume it. Clear the fragment after completion;
use no-referrer and exclude analytics/third-party assets on security routes.

## Recovery and abuse limits

Recovery requests accept an address and return the same public 202 acknowledgement.
Only a currently verified address gets a mail job. Public response shape and scheduling
avoid obvious existence/timing differences; this is not a claim of mathematical
constant-time behavior under all dependency conditions.

Use existing distributed rate limiting: current-password operations allow 5 attempts
per user per minute; enrollment allows 3 per user and target address per 15 minutes;
recovery allows 10 per IP and 3 per target address per 15 minutes. Hash normalized
targets in limiter keys. Target throttling retains the generic 202 response; IP
throttling may return the existing 429. Failed limiter dependencies retain existing
fail-closed behavior. Requests never lock a user account out of normal login.

Permit up to three live reset challenges per account. An anonymous resend does not
invalidate usable links; rate/active limits suppress additional issuance. A completed
reset invalidates all remaining challenges. Reset links use
`/reset-password#token=<opaque-token>` and POST the token with the new password.

Hash the new password through the bounded service, then lock user and challenge in
the documented order. Recheck token digest, purpose, expiry, consumption, versions
and verified address. Atomically consume, change password, invalidate challenges,
revoke sessions and append disconnect jobs. Invalid passwords or a failed commit do
not burn the token. A lost successful response is handled by normal login with the
new password; repeating a consumed token does not mutate credentials again.

## Durable mail and configuration

Select `lettre` 0.11.23 with Tokio/rustls support and a scoped compatibility check
before adding it. Use a library AES-256-GCM implementation for replayable mail
payloads, with a fresh random 96-bit nonce and job ID/purpose as authenticated data.
No handwritten cryptography. Challenge rows retain only token digests; the independent
mail worker decrypts the protected payload to produce the link.

Configuration is `MAIL_ENABLED` (default false), `APP_PUBLIC_ORIGIN`, `SMTP_HOST`,
`SMTP_PORT`, `SMTP_TLS_MODE` (`starttls` or `implicit`), `SMTP_USERNAME`,
`SMTP_PASSWORD`, `SMTP_FROM` and `MAIL_QUEUE_ENCRYPTION_KEY` (32-byte base64 key).
Production links require HTTPS and a configured exact application origin. Never
derive links from request Host headers. Required TLS validates certificates; no
opportunistic downgrade. Explicit invalid enabled configuration fails startup with
redacted diagnostics; absent mail configuration leaves existing text/auth usable.

Use a 10-second total SMTP deadline, 30-second leases and token-fenced completion.
Retry transient failures with jittered exponential delays capped at 60 seconds until
the job/challenge deadline; cancel expired/invalidated challenges. Handle dependency
failure inside the mail loop so it cannot terminate realtime/media workers. Claims
and network sends are separated; no SQL locks remain held during delivery.

An interrupted post-send acknowledgement may repeat the same email/link; token
consumption remains single-use. Remove encrypted payloads on terminal completion or
expiry. Preserve the queue key with backups; drain pending jobs before replacing a
single configured key. Log only stable failure categories and job IDs. SMTP acceptance
is distinct from actual mailbox arrival.

## Verification and delivery

Real Rust HTTP/PostgreSQL tests cover wrong password, active-session revalidation,
concurrent login/change/reset, all-device revocation and outbox rollback. Email tests
cover competing ownership, stale versions, expired/used tokens, resend behavior,
normalization, GET scanners, token secrecy and atomic rollback.

Use an owned local SMTP capture fixture for real protocol delivery, retry, timeout
and restart tests. The fixture's test-only transport/certificate configuration must
not relax production TLS. Browser tests exercise actual captured verification/reset
links, inline sign-in, Chinese forms, session/cache teardown and two-device rejection.
Regenerate API, run root integration checks, and separately test a consenting staging
mailbox with operator-provided SMTP configuration. No real user mail is sent during
local fixtures. Record external mailbox/staging checks as pending until executed.

References: [SMTP transport](https://docs.rs/lettre/latest/lettre/transport/smtp/struct.AsyncSmtpTransport.html),
[email identity](https://cheatsheetseries.owasp.org/cheatsheets/Email_Validation_and_Verification_Cheat_Sheet.html),
[recovery guidance](https://cheatsheetseries.owasp.org/cheatsheets/Forgot_Password_Cheat_Sheet.html),
and [AES-GCM library](https://docs.rs/aes-gcm/latest/aes_gcm/).
