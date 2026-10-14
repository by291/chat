# PWA and Background Push Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans after execution-method selection. Steps use checkbox syntax.

**Goal:** Install the existing SPA and deliver private generic background notifications
through a separately leased workflow, with safe updates and authenticated device ownership.

**Architecture:** The service-worker entry composes static asset transport and push
presentation. Authenticated feature owners handle subscription/activity lifecycle.
Rust notifications owns durable policy/SQL, while a concrete provider adapter owns
encryption and bounded network delivery independently of realtime/media/mail.

**Tech Stack:** SvelteKit 3's native Service Worker/manifest interfaces, existing
Query/session owners, Rust/SQLx, `web-push-native =0.5.0`, reqwest transport, real fixtures.

**Spec:** [Approved PWA/push design](../specs/2026-10-14-pwa-push-design.md).

**Local status (2026-10-15):** Implemented and verified by the final combined
`node scripts/check.mjs --integration` gate (exit 0). See the
[acceptance record](../../operations/product-readiness-acceptance.md) for source
identity, counts, review follow-up and external checks that remain pending.

## Global Constraints

- No private API/media/credential/token caching; only explicitly allowed static assets and generic offline content.
- User-controlled worker activation/reload; do not force away drafts/uploads/submissions.
- A session has at most one subscription; an account at most ten; bind using the server actor.
- Activity heartbeat is ten seconds, lease thirty seconds; first dispatch waits five seconds and expires after five minutes.
- Provider deadline ten seconds, queue lease thirty seconds, retry delay capped at sixty seconds.
- Only new incoming messages enqueue push; notifications never advance event/read cursors.
- Generic visible text, same-origin allowed click paths, no body/sender/group/attachment content.
- iPhone/iPad real-device acceptance is deferred; local/provider tests do not imply OS presentation.
- Use the [owned verification recipe](2026-10-14-product-readiness-execution.md#targeted-verification-commands); no extra Compose files or unsolicited publish actions.

## Review Focus

1. Production worker output differs from Vite development: Task 1 serves built assets in an owned preview fixture.
2. An update can land during a draft/upload: Task 2 proves no forced reload or takeover.
3. The same browser endpoint can change account: Tasks 3/5 test generation rebinding and queued-work rejection.
4. DNS/redirects can target private hosts: Task 4 tests validated address pinning and no redirects.
5. A close/blur lease can be stale: Tasks 4/5 test expiry, defer/recheck and no read acknowledgement.

### Task 1: production service worker, manifest and safe asset cache

**Files:** Create `web/static/manifest.webmanifest`, standard/maskable PNG icons,
`web/static/apple-touch-icon.png`, `web/static/offline.html`,
`web/src/service-worker/{index.ts,tsconfig.json}` and
`web/src/lib/api-client/asset-cache.ts`.
Modify `web/src/app.html`, `web/vite.config.ts`, `web/tsconfig.json`,
`web/package.json`, `scripts/check.mjs`, `web/tests/integration/runtime.ts`,
`web/tests/unit/check-scope.test.ts`; create unit `pwa-cache.test.ts` and
`web/e2e/pwa.spec.ts`.

**Interfaces:** Shared transport produces
`createAssetCache({ version, assetPaths, cacheStorage, fetchAsset }): AssetCache`,
with `install(): Promise<void>`, `activate(): Promise<void>`, and
`respond(request: Request): Promise<Response | undefined>`.
Use an explicit same-origin asset allowlist; `undefined` means bypass the worker.
Worker code imports SvelteKit 3 `$app/service-worker`, `$app/env` and `$app/manifest`.
Produce `TestRuntime.startWeb(options?: { mode: 'dev' | 'preview' })` with default dev;
preview builds/serves owned production assets against that fixture's API/media.

- [x] Write cache tests for a listed immutable asset, unlisted GET, private API, media URL, mutation, security route and unrelated origin cache. Assertion fragments after constructing the cache with an owned fake CacheStorage:

```ts
expect(await cache.respond(privateApiRequest)).toBeUndefined();
expect(await cache.respond(signedMediaRequest)).toBeUndefined();
expect(savedKeys).toEqual(allowedStaticPaths);
expect(unrelatedOriginCacheDeleted).toBe(false);
```

- [x] Run `pnpm -C web exec vitest run tests/unit/pwa-cache.test.ts`. Expected RED: the cache transport/allowlist behavior is absent.
- [x] Implement the shared static transport and worker composition. Cache only build assets, public icons and generic offline content; navigation tries the network and falls back only to the generic page. Never cache arbitrary successful responses.
- [x] Add the stable root-scope manifest, `/app` start URL, standalone display and product-derived 192/512 icons plus Apple icon. Include manifest/icon document links; preserve the existing mark rather than creating a new visual identity.
- [x] Give the worker its separate TypeScript configuration and add `check:worker`/equivalent no-emit command to `scripts/check.mjs` frontend scope. Update the existing no-Rust/no-Docker scope fixture to include this tool phase.
- [x] Extend the fixture's preview mode to build into its owned `.local/tests/run-*` directory with the correct test media origin, then serve through Vite preview's configured API/WebSocket proxy. If a build-output environment override is needed, restrict it to verified owned test paths and preserve default `web/build` output.
- [x] Route same-origin app navigation through network-first/generic-offline handling while unlisted resource GETs bypass the worker. Security pages retain their explicit bypass; offline navigation must not fall back to cached authenticated HTML.
- [x] Write real browser checks against preview, not development: registration exists, manifest/icon paths work, static assets can load offline, and CacheStorage contains no private responses or transfer grants after actual auth/media use.
- [x] Run units, `pnpm -C web check`, `pnpm -C web check:worker`, and `pnpm -C web exec playwright test e2e/pwa.spec.ts`. Expected GREEN; build defaults and existing dev browser fixtures remain intact.

### Task 2: installation guidance and safe update lifecycle

**Files:** Create `web/src/lib/features/pwa/{lifecycle.svelte.ts,InstallHelp.svelte,UpdateNotice.svelte}`;
modify root/app layout composition and related feature styles.
Create `web/tests/unit/pwa-lifecycle.test.ts`; extend `web/e2e/pwa.spec.ts`.

**Interfaces:** Produce `PwaLifecycle` with `start()`, `stop()`,
`requestUpdate({ hasDraft, hasUpload, hasSubmission }): Promise<void>` and observable
install/update state. The authenticated owner provides pending-work flags; PWA state
does not copy messages or own chat drafts. An explicit update request sends an
activation message to the waiting worker and reloads only after controlled handoff.

- [x] Write lifecycle tests for deferred install prompts, waiting worker, draft/upload guards, cancelled update and teardown. Assertion fragments:

```ts
expect(reloadCalls).toBe(0);
expect(waitingWorkerActivationCalls).toBe(0);
expect(confirmationRequired).toBe(true);
```

- [x] Run `pnpm -C web exec vitest run tests/unit/pwa-lifecycle.test.ts`. Expected RED: no owner controls these transitions.
- [x] Implement browser-only listeners with cleanup, Chinese installation help and an update notice using existing controls. Unsupported install APIs use instructions; do not automatically request notification permissions here.
- [x] Handle the explicit confirmed-update action; no automatic `skipWaiting` or forced `clients.claim` takeover. Retain all ongoing work until that action and warn before the chosen reload discards unsent work.
- [x] Extend the owned preview fixture to serve two actual build versions. Test that a waiting update leaves a draft/upload unchanged, cancel preserves it, and confirmed reload activates the new version and removes only obsolete app asset caches.
- [x] Run unit/worker type/browser targets and frontend scope. Expected GREEN with no update-triggered authenticated lifetime recreation.

### Task 3: authenticated subscriptions and activity leases

**Files:** Create `migrations/0014_push_notifications.sql`,
`src/notifications/{mod.rs,model.rs,http.rs,service.rs,repository.rs}`;
modify `src/{lib,app}.rs`, `src/infrastructure/config.rs`, explicit shared ID/DTO
facades and `src/bin/export_openapi.rs`.
Create `tests/push_subscriptions_test.rs` and unit/config coverage.

**Interfaces:** Define `PushSubscriptionId`, `SubscriptionInput { endpoint, p256dh, auth }`,
`SubscriptionBinding { id, generation }`, `PushConfigView { enabled, vapid_public_key }`
and `ActivityInput { visible, focused, conversation_id }`.
Service boundaries are `register_subscription(state, actor, input) -> Result<SubscriptionBinding, AppError>`,
`remove_subscription(state, actor, id) -> Result<(), AppError>` and
`record_activity(state, actor, id, input) -> Result<(), AppError>`.
Repository records contain authoritative session/owner/binding/enable time; they never commit.

- [x] Write HTTP/database tests for disabled config, foreign binding, same-session idempotency, one/ten caps, malformed keys, new-account endpoint rebind and thirty-second activity expiry. Assertion fragment after controlled actor requests:

```rust
assert_eq!(first_binding.id, repeated_binding.id);
assert_ne!(old_binding.generation, rebound_binding.generation);
assert!(!old_binding_active);
assert_eq!(foreign_remove.status(), 404);
assert!(!client_supplied_identity_field_effect);
assert!(victim_binding_active_after_foreign_capability_mismatch);
```

- [x] Run Rust target `push_subscriptions_test`. Expected RED: new endpoints/contracts are absent.
- [x] Implement the migration's constrained subscriptions/activity/jobs, unique active session/endpoint identity and event/subscription job identity. Serialize competing rebinds so only one active generation remains.
- [x] Implement authenticated endpoints and Origin/error mapping from the spec. Actor/session validity, caps, key sizes and endpoint scheme/host checks precede persistence; no caller-supplied identity is authoritative.
- [x] For an already-known endpoint, compare its submitted public/auth capability material with the stored binding before cross-session/account reassignment. Format-valid arbitrary keys must not let another account disable a victim's binding; a mismatch returns conflict and leaves that binding unchanged.
- [x] Implement leased activity timestamps from the server clock and validate current conversation membership. This operation must neither query/apply event pages nor update receipt cursors.
- [x] Add default-disabled configuration and regenerate API. Test expired/revoked sessions and account changes alongside existing auth endpoints.
- [x] Run subscription/config tests, existing auth tests and generated contract check. Expected GREEN with no credential/key/endpoint leakage into public summaries or diagnostics.

### Task 4: bounded provider adapter and durable new-message queue

**Files:** Create `src/infrastructure/web_push.rs`,
`src/notifications/{delivery.rs,worker.rs}`;
extend notification repository/service, `src/conversations/messages.rs`,
worker/AppState composition and `.env.example`.
Create `tests/support/push.rs`, `tests/push_delivery_test.rs` and
`web/tests/integration/push-support.ts`;
extend worker dependency tests and runtime fixtures.

**Interfaces:** Produce `WebPushDelivery` and typed `LeasedPushJob`/`PushOutcome`.
`enqueue_message_push(tx, sender: UserId, conversation: ConversationId, message: MessageId, envelope: &EventEnvelope) -> Result<(), AppError>`
is called only after a new message event, before its caller commits.
`delivery::deliver(state, job: &LeasedPushJob) -> Result<PushOutcome, AppError>`
rechecks authorization; `worker::run_worker(state, cancel) -> Result<(), AppError>`
owns claim/defer/retry/CAS. `PushOutcome` distinguishes accepted, deferred, skipped,
expired binding, retryable provider failure and definitive failure.

- [x] Write actual signing/encryption and queue tests using generated test subscription keys and an owned TLS provider fixture. Include DNS/private IP/redirect rejection, stale lease, 404/410/429/5xx, deadline, withdrawn/read/muted messages, rejoin, provider outage and post-acceptance crash. Assertion fragments:

```rust
assert_eq!(decrypted_payload["schema_version"], 1);
assert!(decrypted_payload.get("message_body").is_none());
assert_eq!(push_jobs_for_duplicate_send, 1);
assert_eq!(push_jobs_for_edit, 0);
assert!(realtime_message_delivered_during_push_timeout);
```

- [x] Run Rust target `push_delivery_test`. Expected RED: adapter/queue/enqueue boundaries are absent.
- [x] Pin `web-push-native =0.5.0` for signing/encryption/request building. Compile and construct it with the existing runtime/TLS adapters; bridge its generated request to bounded reqwest without introducing unchecked redirects or handwritten crypto. The documented compatibility ruling replaces the original native OpenSSL candidate.
- [x] Validate provider host allowlisting and DNS results, reject all nonpublic destinations and pin the checked socket addresses while preserving TLS hostname verification. The local fixture uses an explicitly test-only transport path; production config never disables these checks.
- [x] Provide the Rust-owned TLS provider in `tests/support/push.rs` and export `TestPushProvider` in `web/tests/integration/push-support.ts` with `start(): Promise<void>`, `requests(): CapturedPush[]`, `stop(): Promise<void>` and test subscription key generation/decryption. `CapturedPush` retains the fixture endpoint, status and decrypted payload; fixture owners close sockets and delete only their own files. Pass private fixture configuration to TestRuntime. Gate endpoint substitution with `test-failpoints` and prove the default-feature application retains all endpoint restrictions.
- [x] Enqueue one eligible job per current recipient binding/episode in the new message transaction. A same-ID retry does not enqueue again; devices enrolled after creation do not acquire old jobs. Recheck session/generation/member/message/preferences/read state before dispatch.
- [x] Add explicit sender-exclusion, late-enrollment and archive-without-mute cases. Assert only recipients enrolled at creation receive jobs and archiving alone does not suppress an unread incoming message.
- [x] Implement five-second initial delay, five-minute deadline, ten-second network deadline and thirty-second CAS lease. A matching foreground lease defers until expiry; it never permanently discards an unread job. Retry-After cannot exceed the job deadline; definitive subscription expiry deactivates the binding.
- [x] Compose a separate cancellation-aware loop whose dependency failures do not propagate into unrelated workers. Use generic private-content-free payloads, stable tags and a relative allowed click route. Preserve queue state across restart and map failures to redacted categories.
- [x] Run queue/adapter/dependency targets and backend scope. Expected GREEN: real encrypted requests, atomic enqueue rollback, provider outage isolation and no event/read advancement by push.

### Task 5: enrollment controls, session fencing and push presentation

**Files:** Create `web/src/lib/features/notifications/{api.ts,subscriptions.svelte.ts,BackgroundPushSettings.svelte}`
and `web/src/service-worker/push.ts`.
Modify notification settings, authenticated layout/session cleanup and worker composition;
create units `push-subscriptions.test.ts`/`push-presentation.test.ts` and
`web/e2e/background-push.spec.ts`.

**Interfaces:** Produce `createNotificationApi(client)` for the generated contracts
and `PushSubscriptions` with `start()`, `enable()`, `disable()`, `setActivity(...)`,
`stop()`. All requests/timers bind to one authenticated generation. Worker presentation
produces `showPush(registration, payload)` and `openNotification(clients, payload)`;
both validate the version/tag/path and never fetch/apply chat data.

- [x] Write tests for denied/unsupported permission, explicit request gesture, rebind after account change, stale enable responses, blur/close lease release, timer teardown, malformed push fallback and unsafe click paths. Assertion fragments:

```ts
expect(permissionRequestsBeforeClick).toBe(0);
expect(oldSessionSubscriptionsApplied).toBe(0);
expect(receiptAcknowledgements).toBe(0);
expect(openedUrls.every((url) => new URL(url).origin === appOrigin)).toBe(true);
```

- [x] Run the two unit targets. Expected RED: subscription owner and worker presentation do not exist.
- [x] Implement typed registration/removal/activity through the feature adapter. Keep capability material private; disable/logout clears timers/listeners/UI and attempts browser unsubscribe without bypassing server ownership.
- [x] Separate open-page controls from optional background push in Chinese UI. Use user gestures for permission and installation guidance/fallback where needed. Renew visible/focused activity every ten seconds; no current-chat hint becomes a read receipt.
- [x] Implement generic visible notifications for accepted/malformed push, stable replacement tags, allowed same-origin focus/open behavior and authenticated route restoration. Close prior visible notifications on disable/logout; document already accepted provider-race limitations.
- [x] Add preview-mode browser scenarios with controlled browser permission/subscription capabilities but real REST/worker/provider fixture. Cover another account/device, logout/rebind, mute/read/context navigation, cache privacy and no duplicate durable message application.
- [x] Run units, both frontend typechecks and `pnpm -C web exec playwright test e2e/background-push.spec.ts`. Expected GREEN. Controlled capability tests remain distinct from real OS/provider acceptance.

### Task 6: integrated worker/PWA recovery and operations

**Files:** Update architecture and `docs/operations/pwa-notifications.md`,
deployment/development config examples, production preview fixtures and
the acceptance record in the coordinator plan.

**Interfaces:** Consume the completed optional adapters, authenticated lifetime,
durable message/replay model and independently cancelled workers.

- [x] Add upgrade/restart tests: existing accounts are not silently subscribed, VAPID keys remain stable, revoked bindings/jobs stay revoked, and a recovered pending job rechecks current authorization before delivery.
- [x] Run generated API check and `node scripts/check.mjs --integration` on the integrated account/PWA/push revision. Expected GREEN with worker output/type/cache and real queue regressions included.
- [x] Document allowed providers/outbound networking, stable VAPID/contact, TLS/endpoint policy, re-enrollment after key changes, activity delay/expiry, safe updates and provider acceptance versus OS presentation.
- [x] Record available desktop HTTPS/provider observations separately. Preserve deferred iPhone/iPad acceptance and all other unavailable external inputs as pending.
