# SeaweedFS Media Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Send, preview and recover real images/files/avatars with correct scoped authorization.

**Architecture:** A media feature owns scoped upload/readiness and leased processing/cleanup.
An explicit S3 adapter separates private operations from public signing. Candidate keys
are immutable per attempt, and message binding stays inside the conversation transaction.

**Tech Stack:** AWS SDK for Rust, image, SeaweedFS 4.48, existing PostgreSQL/worker/Svelte.

**Spec:** `docs/superpowers/specs/2026-10-13-seaweedfs-media-design.md`

## Global Constraints

- Root Rust and `web/`; English code/docs, Chinese product UI, generated Rust-owned API.
- 50 MiB = 52,428,800 bytes; ten attachments/message; ten pending uploads/user.
- Default quota 1 GiB; PUT grants ten minutes, GET grants 60 seconds.
- Images: JPEG/PNG/WebP; dimensions ≤8192 each and ≤20 million pixels; thumbnail ≤512 each.
- Two blocking processors; best-effort 128 MiB image allocation limit, not a process-memory guarantee.
- Expire unfinished uploads after one hour; unused ready objects/avatars retained 24 hours.
- No permanent PUT grants; current membership/join visibility and avatar policy control downloads.
- No commit/push/public deployment without explicit request.

## Review Focus

- An old PUT or stale processing lease never changes a published permanent object.
- Cleanup racing a send/avatar swap cannot delete newly referenced bytes.
- Removed/rejoined members cannot obtain pre-join files using known IDs or ownership.
- A canceled blocking decode holds its permit until it exits and cannot publish late results.
- Image expansion must not acknowledge a timeline the user has not visibly rendered.

## File Structure

- New media migration after groups: attachment/attempt/job/object/reference/quota records.
- `src/media/{mod,model,http,service,repository,processing,worker}.rs` owns media workflows.
- `infrastructure/object_storage.rs` owns bounded SDK operations/public signing.
- App/config/types/bin worker/OpenAPI wire concrete dependencies and cancellation.
- Conversation message/repository and user profile owners compose ready references.
- `web/src/lib/features/media/{api,models,uploads.svelte}.ts` owns session upload state;
  shared signed-XHR transport owns progress/abort mechanics. Chat/settings consume it.
- Compose/Caddy/CSP/scripts/backup/restore own origins, ingress and joint recovery.

### Task 1: Scoped upload, immutable processing and cleanup

**Files:** migration, media feature, object storage, config/app/types/worker; media Rust/integration tests.

**Interfaces:**
- `AttachmentId`, `UploadAttemptId`; purpose `message|avatar`, state
  `uploading|processing|ready|failed|expired`.
- `UploadInput {purpose, conversation_id?, membership_id?, filename, content_type, size_bytes}`.
- `UploadGrant {attachment:AttachmentView, attempt_id, url, headers, expires_at}`;
  complete body `{attempt_id}`; metadata never exposes keys/credentials/lease tokens.
- `AttachmentView {id,purpose,state,filename,size_bytes,content_type,width?,height?,sha256?,error_code?}`.
- Download response `{url,headers,expires_at}`, variant `original|thumbnail`.
- Service endpoints: `/media/uploads`, `/{id}/upload-url`, `/{id}/complete`, `/{id}`
  (GET/DELETE), `/{id}/download` as in the spec.
- Concrete storage adapter: signed upload/download plus bounded head/copy/read/write/delete.
- Media worker: independent cancellation-aware loop with leased CAS; operations time out
  at ten seconds, renewed processing leases last two minutes, retry delay caps at 60 seconds.

- [ ] Add tests for upload cap/quota/purpose/member/attempt fences, completion idempotency,
  malformed images/dimensions, per-attempt keys and stale-lease publication; expect RED.
- [ ] Run targeted Rust/media tests before implementation and preserve the actual failure.
- [ ] Add tracked attempts/candidates, ordered message references, user avatar reference,
  leased media jobs and quota reservations through a new migration. Record candidate keys
  before storage writes. Tombstone deletion under locks before issuing network deletion.
- [ ] Implement typed config with explicit internal/public endpoints, bucket, region and
  credentials; minimal SDK/image features compatible with Rust 1.99, named limits,
  bounded calls and custom endpoint/path style. Do not rewrite signed Host/path afterward.
- [ ] Implement grant renewal with new staging attempt, seal/HEAD, immutable candidate
  validation, safe thumbnail output, live-lease publication and retryable cleanup.
  Aborted/failed reservations release once after safe cleanup and outstanding PUT expiry.
- [ ] Run targeted tests with actual SeaweedFS, including overwrite-after-ready,
  stale worker and storage outage; expect all green and uninterrupted realtime delivery.

### Task 2: Atomic message and avatar association

**Files:** conversations model/messages/repository/events; users model/service/repository/http;
app/OpenAPI/generated contracts; Rust message/avatar permission tests.

**Interfaces:**
- Send input gains default-empty `attachment_ids`; MessageView/event decoding gains
  default-empty `attachments:Vec<AttachmentView>` for historical text records.
- `media::service::bind_message(tx, actor, membership, conversation_id, message_id, ids)`
  validates readiness/ownership/scope, locks attachment IDs in sorted order, preserves
  requested display order, and adds references without committing caller transaction.
- User routes: PUT/DELETE `/users/me/avatar`, GET `/users/{id}/avatar`; pointer swap is
  atomic and cleanup rechecks all message/avatar references before deleting.

- [ ] Write regression tests for file-only send, wrong owner/conversation/episode,
  non-ready binding, exact/different-ID retries, rollback and old JSON payload defaults;
  plus avatar format/visibility/replacement cleanup races. Expect RED.
- [ ] Implement conversation-first then sorted-attachment lock order. Compare frozen
  body+ordered IDs for idempotency. Hydrate identical stable metadata in every path.
- [ ] Implement profile-owned avatar selection/clear/grants and safe Content-Disposition;
  generic files are attachment/octet-stream/nosniff and grants are private/no-store.
- [ ] Regenerate API and run affected Rust tests; expect old text and new media cases green.

### Task 3: Browser upload, preview and lifecycle

**Files:** media feature/transport, composer/timeline/coordinator, avatar settings, styles;
frontend unit/browser tests.

**Interfaces:** consume generated Task 1–2 APIs. File/progress/grant/controller state is
session-owned; authoritative attachment/message records live only in the query cache.
Membership changes abort affected uploads and late results. Same-user revalidation preserves drafts.

- [ ] Add unit tests for attempt retries, stale membership/status responses, logout
  cleanup and identical pending-send attachment payloads; expect RED.
- [ ] Implement signed XHR PUT with genuine progress/abort and exact grant headers,
  typed create/renew/complete/status/download adapter, bounded status polling and blob cleanup.
- [ ] Implement choose/cancel/retry/processing/ready states, file-only sends, safe previews
  and downloads, avatar upload/clear. Reserve dimensions and reconfirm actual rendered
  visibility after image load/resize; do not acknowledge merely fetched media.
- [ ] Run frontend types/unit tests and real-browser signed upload progress/CORS/retry,
  mobile scrolling and removed/rejoined download denial; expect all green.

### Task 4: Media origins, recovery and full acceptance

**Files:** all relevant Compose/Caddy/CSP/Docker/env example configs; dev/e2e/production/
backup/restore scripts; real media/restore/deployment/browser suites; docs and CI coverage.

**Interfaces:** internal S3 stays private; browser origin is explicit, separate and
build/runtime-consistent. Public object methods/paths are restricted and PUT is capped.
Backups coordinate all writers and media ingress at a common recovery point.

- [ ] Write real separate-origin signed PUT/GET, size/CORS/CSP/admin-denial and coordinated
  restore cases before declaring deployed behavior; expect failures with old wiring.
- [ ] Wire local/e2e/production media origins, exact CORS/CSP, public body cap, bucket
  creation and permissions. Local credential updates preserve existing random secrets.
  Keep unsupported/list/admin routes private and do not log signed URLs.
- [ ] Implement maintenance/drain and backup-finally recovery; restore original files,
  thumbnails/avatars, associations and pending jobs in a fresh isolated stack.
- [ ] Extend Linux CI storage prerequisites/portable test startup as needed by new tests;
  preserve the shared check entry and document actual automated vs local coverage.
- [ ] Run `./scripts/check.ps1 -IncludeIntegration`, rebuilt
  `./scripts/production-test.ps1 -Prepare`, and `pnpm -C web test:deployment`; expect
  all baseline/new cases green. Inspect desktop/mobile media UI and clean only owned resources.
- [ ] Record exact test counts/results, version pins, limits, deployment and recovery
  commands in architecture/operations/acceptance docs. Obtain one fresh full review,
  fix material findings via RED→GREEN and finish with a clean scoped diff.
