# Account security and mail recovery

Username/password registration and login remain available without mail. Settings
allows password changes after entering the current password. Success increments the
credential version, invalidates security links and revokes every device session in
one transaction. Incorrect current passwords leave the session valid. Passwords
retain the existing 6–128 Unicode-scalar policy. Users log in again after a change.

The successful change does not overwrite browser cookies: a delayed response must
not delete a newer login made in another tab. The old HttpOnly cookie is rejected
by durable session revocation and is replaced by the next login. The client clears
only the matching authenticated lifetime.

## Optional mail configuration

Mail is disabled by default. Set `MAIL_ENABLED=true` in protected API and worker
configuration together with the `MAIL_*` and `SMTP_*` fields in `.env.example`.
`APP_PUBLIC_ORIGIN` must be the exact HTTPS application origin, without a path,
credentials, query or fragment. Links use that origin, never incoming Host headers.
Production supports certificate-verified implicit TLS or required STARTTLS without
plaintext downgrade. The sender must be a valid mailbox. Invalid enabled settings
reject startup with redacted diagnostics; disabled mail preserves existing auth.
Complete the selected provider's sender/domain verification before production use;
SMTP credentials alone do not authorize an arbitrary From address.

`MAIL_QUEUE_ENCRYPTION_KEY` is a standard Base64-encoded random 32-byte key. Queue
payloads use AES-256-GCM, a fresh nonce and authenticated job ID/purpose. Keep the
key, SMTP credentials, recipients, tokens and plaintext payloads out of logs.

Mail runs independently in the existing worker. Queue leases last 30 seconds; SMTP
has a total 10-second deadline. Transient retries use jitter capped at 60 seconds
and job expiry. Terminal jobs erase ciphertext. SMTP acceptance confirms provider
handoff, not mailbox arrival. Check actual receipt and spam folders separately.

## Verified recovery email

Only the account owner can read settings email state. Enrollment requires the
current password and a still-active initiating session. The old verified mailbox
remains authoritative until explicit confirmation. Pending claims do not reserve
an address; confirmed ownership is unique.

Parsing trims whitespace and lowercases the domain. Local-part case, dots and plus
suffixes remain intact; no provider-specific alias folding occurs. The entered
address is retained for display/delivery. `Alice@example.test` and
`alice@example.test` are deliberately different canonical claims.

Verification links expire after 30 minutes. Confirm while signed in as the
initiating account; a wrong actor leaves the link usable. Inline sign-in stays on
the fragment route. Resending replaces the previous pending verification. Success
increments the email version, invalidates other security links and sends a 24-hour
notice to the old address when it changes. Email confirmation preserves sessions.

Current-password checks allow five attempts per account per minute. Enrollment
allows three requests per account and canonical target in 15 minutes. Rate-limit
keys hash identifiers, and dependency failures fail closed.

## Recovery and one-time reset

The login page links to `/forgot-password`. Known, unknown, unverified and
target-throttled mailboxes receive the same empty 202 response. Only verified
addresses receive mail. Limits are ten requests per trusted client IP and three
per canonical target in 15 minutes. At most three unexpired reset links are live;
an anonymous resend preserves the earlier usable links.

Reset links expire after 15 minutes. The browser reads
`/reset-password#token=...`; ordinary GETs and mail scanners cannot consume it.
Explicit submission rechecks the mailbox and credential/email versions under user
and challenge locks. Consuming the link, changing the hash/version, invalidating
other challenges and revoking/disconnecting every device commit together.
Concurrent resets have one winner. Invalid passwords and failed transactions keep
the link usable. Reset neither logs in nor issues a cookie.

Tokens have 256 random bits; challenge storage keeps SHA-256 digests only. Raw
tokens occur in encrypted mail and transient route state. They never enter query
redirects, localStorage or sessionStorage. Security API responses use `no-store`;
pages use `no-referrer`. Completion removes the fragment from history.

After a lost submission response, first try the new password. A committed reset
makes the old token unusable; an uncommitted/expired request can be retried through
a fresh mail. Accounts without a verified mailbox or mailbox access require the
existing administrator `reset-password` command. It uses the same version/link
invalidation and all-device revocation boundary.

## Backup, restore and key rotation

Back up PostgreSQL with the protected encryption key and existing service secrets.
Restored pending jobs need the exact original key. The local coordinated drill
restores actual encrypted verification/reset jobs, delivers their original links
and checks revoked sessions remain rejected.

Before key rotation, stop accepting intents and drain pending jobs, or explicitly
cancel them and request fresh links. Replacing the key with pending old jobs makes
their payloads undecryptable. Do not export a plaintext queue for rotation.
Loopback capture and custom trust exist only in test-failpoints builds; production
has no fixture bypass.

Local tests cover real PostgreSQL, TLS SMTP, queue recovery and Chromium flows.
Provider mailbox arrival, HTTPS staging and hosted CI for new source require
operator inputs and are recorded separately from local acceptance.
