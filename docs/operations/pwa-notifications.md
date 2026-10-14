# Installation, updates and background notifications

The production frontend exposes a root-scope manifest, public icons and a Service
Worker. Desktop browsers can offer installation; iPhone/iPad users add the site to
their home screen and open that installed app. Permission is requested only through
the background-push settings button. Unsupported or denied environments keep the
existing in-page feedback. Installation does not enable push automatically.

## Static cache and updates

The worker caches only explicitly listed immutable build assets, public icons,
the manifest and generic offline content. API responses, authenticated HTML, security
routes and media transfers bypass its cache. Offline app navigation shows a generic
page rather than saved chat content. Activation removes obsolete application asset
caches and preserves unrelated caches at the origin.

A new worker waits. The authenticated update notice warns about drafts, uploads and
submissions; cancelling preserves current work. Confirming activates the chosen
worker and reloads after controller handoff. Unsent work lives in the current page
and can be discarded by that confirmed reload. There is no automatic takeover.

## Optional provider configuration

API and worker use the same protected environment. Push is disabled by default:

```dotenv
WEB_PUSH_ENABLED=false
WEB_PUSH_PRIVATE_KEY=<32-byte base64url private scalar>
WEB_PUSH_PUBLIC_KEY=<65-byte uncompressed P-256 public key, base64url>
WEB_PUSH_CONTACT=mailto:operator@example.com
WEB_PUSH_ALLOWED_HOSTS=<exact browser-provider DNS hosts, comma-separated>
```

Enabled startup requires matching keys, a valid contact and exact provider hosts.
Preserve keys across restart, upgrade and recovery. A changed VAPID key requires
disabling and re-enabling browser subscriptions; retaining an old browser capability
does not migrate its provider authorization.

This Node fragment generates a matching pair into a new private file without
printing secrets. Run it from the repository root after creating `.local/`, then
transfer its values into the operator's protected configuration and add the contact
and provider hosts. `wx` refuses to overwrite an existing file.

```javascript
import { createECDH } from "node:crypto";
import { writeFileSync } from "node:fs";

const key = createECDH("prime256v1");
key.generateKeys();

// Node may omit leading zeros; VAPID requires a fixed-width P-256 scalar.
const privateKey = Buffer.alloc(32);
const scalar = key.getPrivateKey();
scalar.copy(privateKey, privateKey.length - scalar.length);

writeFileSync(
  ".local/web-push.env",
  [
    "WEB_PUSH_PRIVATE_KEY=" + privateKey.toString("base64url"),
    "WEB_PUSH_PUBLIC_KEY=" + key.getPublicKey().toString("base64url"),
    "",
  ].join("\n"),
  { flag: "wx", mode: 0o600 },
);
```

Signing/encryption use `web-push-native` 0.5.0 with its pure-Rust VAPID feature;
the production adapter adds no OpenSSL or Perl build requirement. Existing Rust
platform prerequisites still apply. HTTPS and mail TLS remain enabled as required.

## Network and delivery policy

Allow the worker outbound HTTPS to the actual browser provider hosts used by the
installation. Hostnames are exact allowlist entries, with no wildcards, IP literals,
userinfo, fragments or nonstandard ports. The adapter permits only HTTPS port 443,
validates every DNS address as public unicast, pins that DNS snapshot for the request
and retains TLS hostname verification. Redirects and environment proxies are
disabled; a provider redirect is a definitive rejection. Never allow private DNS
destinations as a production workaround.

Only a new incoming message creates a push intent. The message, event, realtime
outbox and push intent commit together; same-ID retries, edits and reactions do not
create another intent. Devices enrolled later receive no backlog. Jobs capture the
recipient's current session, subscription generation and membership episode.

The independent push loop waits five seconds initially and gives a job five minutes
to finish. Each provider call has a ten-second deadline, and workers use thirty-second
fenced leases. Transient errors retry with jitter; Retry-After is honored only within
the job deadline. HTTP 404/410 expires the captured binding. Provider failures do not
stop realtime, media or account mail workers.

Before dispatch, the worker rechecks the session, binding, membership episode,
withdrawal, mute preference and read cursor. Archiving alone does not mute. A visible,
focused page renews its current-conversation hint every ten seconds; hints expire
after thirty seconds. A matching hint defers an unread job until expiry, without
marking a message read or permanently discarding it. Closing/blur releases the hint
when possible; a lost hint or release is bounded by its server expiry.

## Privacy and lifecycle

The encrypted payload contains a schema version, stable event tag, binding generation
and allowed relative conversation route. Visible text is generic: Chat and a new
message notice. Message bodies, names, group titles and attachment details are absent.
Malformed payloads show a generic app-entry notification. Click navigation restores
identity and obtains currently authorized history through the normal application.
Existing windows receive SPA navigation so drafts are preserved. Push never applies
an event or advances a synchronization/read cursor.

One authenticated owner holds browser capability handles, requests, listeners and
timers. Startup waits up to fifteen seconds for an active worker, including the
first registration after page load; cancellation fences old readiness callbacks.
An installation failure is reported without an unbounded wait. Ordinary SPA route
disposal clears local work while preserving device consent for the next owner.
Disable removes the owned server binding and unsubscribes the browser; logout
also closes presented notifications and fences late callbacks. Server session
revocation remains authoritative if optional browser cleanup fails. Already accepted
provider messages may arrive after logout; they remain generic and clicking still
requires current authorization. Duplicate provider acceptance after a worker crash
can occur, with the same replacement tag; OS presentation is not exactly once.

Protect database/backups and VAPID configuration: subscription endpoints and auth
material are private capabilities. Public config exposes only the enabled flag and
public VAPID key. Diagnostics record categories without provider credentials,
endpoint tokens or payload content.

## Verification boundaries

The isolated Rust TLS provider and independent Node receiver verify real VAPID and
decrypt actual RFC 8291 requests. Production preview exercises the built worker,
REST binding and browser lifecycle with controlled subscription capabilities.
The notification scenario uses full Chromium's new headless mode; the default
headless shell lacks this capability. `CHAT_TEST_PUSH_BROWSER_CHANNEL=chrome` or
`msedge` can use an installed browser with a fresh test profile when the pinned
Chromium cannot run locally; CI defaults to its pinned full Chromium.

`CHAT_TEST_PUSH_*` destination/certificate overrides exist only in binaries built
with `test-failpoints`. Their adapter validates an owned loopback destination and
certificate. Production default-feature binaries retain DNS/TLS restrictions and
ignore those variables.

These checks establish application/provider acceptance, not actual external-provider
or OS delivery. Record real HTTPS deployment, provider, browser and OS observations
separately. iPhone/iPad real-device acceptance is explicitly deferred by the user.
