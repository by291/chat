# PWA and background notification design

**Status:** Design approved on 2026-10-14; local implementation and integrated
verification completed on 2026-10-15, including pinned full Chromium push tests.
See the [acceptance record](../../operations/product-readiness-acceptance.md).
External provider/OS acceptance remains pending; iPhone/iPad acceptance is deferred
by the user.

**Goal:** Install the existing static application and notify enrolled devices when
its page is closed/backgrounded, preserving authenticated data and durable replay.

## Choice and ownership

Use SvelteKit 3's built-in service-worker build, a standard Web App Manifest and
standard Web Push/VAPID. A vendor-specific notification SDK or separate application
server would add an unnecessary ownership/deployment boundary. Browser-vendor push
services remain external delivery dependencies; self-hosting does not eliminate them.

PWA browser lifecycle belongs to `web/src/lib/features/pwa/`; notification enrollment
and device activity belong to `features/notifications/`. Typed HTTP lives in each
feature's `api.ts`. Service-worker asset network/cache operations use shared transport
under `api-client`; shared code does not import features. The service-worker entry
composes independent static-cache and push/click handlers.

Rust `src/notifications/` owns HTTP/model/service/repository and durable delivery
workflows; `infrastructure/web_push.rs` owns signing/encryption and bounded transport.
The existing worker composes a separate push loop beside realtime, media and mail.
No push-provider request runs inside message transactions or realtime fanout.

## Installation, caching and updates

Manifest identity/scope are stable at the app origin; start URL is `/app`, display
is standalone, and icons include 192/512-pixel standard/maskable variants and an
Apple touch icon derived from the existing product mark. Preserve Chinese UI.

Use `web/src/service-worker/index.ts`, a colocated configuration extending
`$app/tsconfig/service-worker`, and SvelteKit 3's `$app/service-worker`, `$app/env`
and `$app/manifest`. Add a separate no-emit worker typecheck to frontend checks.
Automatic registration installs a worker; activation/update is user-controlled.

Cache an explicit build-version allowlist of immutable JS/CSS, public icons and a
generic offline page. Exclude API/health/WebSocket/media requests, all mutations,
security routes, authenticated records, signed URLs and response-derived cache keys.
Navigation is network-first with only the generic offline fallback. Prune this
application's obsolete asset caches after activation, never arbitrary origin storage.

An update notice waits for an explicit reload action. Preserve in-memory drafts and
active uploads/submissions until that choice; warn before discarding unsent work.
Do not force a new service-worker version to take over a live authenticated session.

## Subscription and activity contracts

Authenticated paths below are under `/api/v1/notifications` and retain Origin/error
contracts. `SubscriptionId` is a UUID and `generation` is an opaque binding UUID.

| Operation                           | Input/result                                                |
| ----------------------------------- | ----------------------------------------------------------- |
| `GET /config`                       | `{ enabled, vapid_public_key }`, nullable key when disabled |
| `POST /subscriptions`               | `{ endpoint, p256dh, auth }` -> `201 { id, generation }`    |
| `DELETE /subscriptions/{id}`        | Owner's current-session binding -> idempotent `204`         |
| `POST /subscriptions/{id}/activity` | `{ visible, focused, conversation_id }` -> `204`            |

Validate encoded keys/lengths and HTTPS endpoints. A session may own at most one
active subscription; an account may own at most ten. A repeated identical request
returns the same active binding. Rebinding the same browser endpoint after a session
or account change disables its old binding and creates a new generation; queued jobs
for that old binding cannot target the new account. Require subscription capability
material and authenticated ownership; never accept a client-provided user/session ID.

Activity is an expiring hint, not proof of receipt/read. An authenticated state owner
renews every ten seconds while the page is visible/focused; leases last thirty seconds.
Hide/blur/logout attempts a bounded explicit release. Validate any conversation ID
against current membership. Multiple tabs sharing one browser subscription may cause
extra generic notifications during races; they must not cause durable cursor changes
or indefinite notification suppression.

## Persistence and delivery eligibility

New immutable tables are `push_subscriptions` (owner/session, binding generation,
private endpoint/key material, enable/revocation time), `push_activity` (binding,
conversation and expiry) and `push_jobs` (event/message/subscription, target membership
episode, generation, deadline, retry and lease fields). Unique event/subscription
identity prevents duplicate job insertion.

After appending a newly created message's event, enqueue applicable push jobs in the
same caller-owned transaction. Capture active recipient membership episodes and
subscriptions already enrolled at message creation; do not replay earlier messages
to devices enabled later. Do not enqueue the author's own devices. A database failure
rolls back message/event/outbox/jobs; external provider failure does not block writes.

Initial dispatch waits five seconds to allow ordinary rendered receipts to settle;
jobs expire five minutes after message creation. Before network dispatch, recheck
binding/session validity, target episode/current authorization, withdrawal, the user's
mute preference and monotonic read cursor. Skip read/muted/unauthorized work. A current
focused-visible lease for that conversation defers until its expiry instead of
permanently discarding the job. Expiry makes the device eligible again; recheck receipt
state before sending. Context navigation must never create a read acknowledgement.

Only new incoming message events enqueue push. Replay backlog, edits, withdrawals,
reactions and preference hints do not. Archive alone does not mute a conversation,
matching the existing preference model. Existing in-page alerts keep their policy.

## Payload, privacy and device lifecycle

Encrypted push content has schema version 1, stable event tag, subscription generation
and a validated same-origin conversation path. Visible text is generic: product name
and a new-message notice, without body, sender name, group title or attachment details.
The worker validates payload/route shape and shows a user-visible notification; a
malformed payload gets a generic app-entry fallback. Do not rely on silent push.

Click handling only focuses/opens a same-origin allowed app route. The application
restores identity and reads current authorized state through its existing adapters;
the notification itself neither applies an event nor advances a read cursor.
Use stable notification tags to replace duplicate attempts, while documenting that
network exactly-once presentation is not guaranteed.

Server delivery rejects revoked/expired sessions independently of browser unsubscribe.
Logout/disable closes existing browser notifications and attempts unsubscribe through
the authenticated owner. Account changes clear notification state and create a fresh
binding. Already provider-accepted messages can arrive afterward; generic payloads
and authorization on click prevent private content access. Do not promise retroactive
withdrawal of accepted or already presented OS notifications.

Settings distinguish existing open-page sound/system alerts from optional background
push. Request permission only from a user gesture; unsupported/denied environments
retain in-page feedback. Show installation guidance when required by the platform.

## Adapter, bounds and configuration

Use `web-push-native` 0.5.0 signing/encryption APIs and its request-builder support
with a transport whose endpoint restrictions can be enforced. A scoped compatibility
check must verify the existing Tokio/Rust/Windows/Linux stack before dependency changes.
No handwritten VAPID/encryption implementation or unconstrained client HTTP redirects.

Compatibility ruling: the original `web-push` candidate pulls Mozilla ECE's OpenSSL
backend, adding native OpenSSL and Perl build prerequisites. The selected replacement
uses RustCrypto and pure-Rust JWT signing. Its isolated Windows build, signed encrypted
round trip and RFC 8291 Appendix A fixture passed without those native dependencies.
Keep the bounded reqwest transport and all subscription/queue contracts unchanged.

Configuration is `WEB_PUSH_ENABLED` (default false), `WEB_PUSH_PUBLIC_KEY`,
`WEB_PUSH_PRIVATE_KEY`, `WEB_PUSH_CONTACT` and `WEB_PUSH_ALLOWED_HOSTS`. Enabled mode
requires a stable matching key pair and valid contact; public config exposes only
the public key. Preserve keys across upgrades and document subscription re-enrollment
if the key pair changes. Missing configuration leaves existing chat/in-page alerts usable.

Endpoint delivery allows only explicitly approved browser-provider hosts. Validate
DNS results and reject loopback/private/link-local/multicast/unspecified destinations;
pin validated addresses for that request and preserve TLS hostname verification.
Disable redirects. A local provider fixture gets an explicit test-only adapter path,
not a production environment switch that bypasses endpoint checks.

Network calls have a ten-second total deadline; jobs have thirty-second leases and
token-fenced completion. Treat 404/410 as expired subscription; 429/5xx/network failure
retry with bounded exponential jitter and Retry-After subject to the job deadline.
Other definitive client failures terminate with a redacted category. Failures remain
inside this loop and do not terminate other workers. Keep endpoints, auth keys,
private VAPID keys and payload URLs out of logs/artifacts.

## Verification and release boundaries

Automated tests cover cache allowlisting, update/draft behavior, worker typechecking,
permission branches, subscription/rebinding ownership, endpoint restrictions, real
encryption/decryption with test keys, queue leases/retries/deadlines, revoke/reset,
read/mute/focus races, group removal/rejoining and click-route authorization.
Real browser scenarios use the existing isolated native stack and a fixture provider.

Actual provider acceptance and OS notification presentation are separate observations.
After operator-provided HTTPS/VAPID configuration, verify available desktop devices
with the page closed/backgrounded, then click into the authorized conversation.
Record provider/browser/OS and browser-running state. iPhone/iPad installation and
system-presentation checks are explicitly deferred by the user. Automated coverage
does not establish that deferred platform result.

References: [SvelteKit service workers](https://svelte.dev/docs/kit/service-workers),
[Push API](https://developer.mozilla.org/en-US/docs/Web/API/Push_API),
[Rust Web Push Native](https://docs.rs/web-push-native/0.5.0/web_push_native/), and
[Apple enrollment](https://webkit.org/blog/13878/).
