# Phase two: SeaweedFS images, files and avatars

Date: 2026-10-13. Status: approved for implementation by the user.
This refines the approved [product design](2026-10-12-chat-design.md) and builds on
[membership episodes](2026-10-13-groups-design.md). CI and groups remain independent
deliverables; this increment makes actual object storage usable from the browser.

## Outcome and boundaries

Users can send images/files in direct chats and groups, upload an avatar, preview
images, and download authorized files. The UI shows progress, processing, failure and
retry. The default object cap is 50 MiB (52,428,800 bytes); at most ten attachments
belong to one message. JPEG, PNG and WebP produce previews; other files are downloads.
Chinese UI, English implementation/docs, root Rust and `web/` remain unchanged.

PostgreSQL owns attachment identity, owner, purpose, conversation/membership scope,
state, immutable object references and message associations. SeaweedFS stores bytes.
Use the maintained AWS SDK for Rust with custom endpoints and path-style addressing.
Keep separate internal operation and public signing clients. S3 credentials stay in
private configuration; browsers receive short-lived signed URLs, never credentials.

Message files and avatars have distinct authorization. Message downloads require
current membership and an associated message visible in that episode. A logged-in
user may fetch an account's current avatar through the same profile visibility as
the existing user lookup. Superseded/unassigned avatars are not public profile media.
File ownership alone does not bypass chat membership or a rejoin boundary.

## Upload and immutable finalization

1. The feature API validates session/Origin, purpose, declared length, filename and
   chat membership. Create a new attachment UUID and independent random staging key.
   Scope chat uploads to the current membership episode. Reserve quota atomically.
2. Return a signed PUT valid for ten minutes. The browser uses XHR through its feature
   adapter to report upload progress and cancellation. Only an uploading attachment
   can renew its grant. Renewal creates a new staging attempt/key and fences the old
   attempt; completion names the selected attempt ID. Keep old attempts tracked for
   cleanup after their grant expires. Failed retries preserve the local draft/file.
3. Completion seals the selected attempt, verifies actual HEAD length, and enqueues
   a durable media job in the same database transaction. It is idempotent and does not promise
   that processing has finished. The browser polls authorized attachment status.
4. The worker copies staging bytes to a new object key unique to its attempt, then
   validates that immutable candidate. A still-valid staging PUT must never change
   bytes already referenced by a sent message. Permanent keys are never granted PUT.
5. Inspect actual format/length rather than trusting MIME or filename. Decode preview
   formats with bounded allocation/dimensions, create sanitized thumbnails, and record
   server-derived metadata. Avatars require a valid supported image. Claimed supported
   images with invalid bytes fail; generic files are served as downloads.
6. Only the current unexpired worker lease may atomically publish ready metadata and
   object references. A stale attempt cannot replace them; its separate objects are
   retained in a cleanup ledger. Retry and completion never expose intermediate keys.

Use upload states `uploading`, `processing`, `ready`, `failed`, `expired`. A successful
send requires every referenced attachment to be ready, caller-owned, in the selected
conversation and current upload episode. Direct uploads use their current member
scope without adding a required field to old text clients. Removal during processing
prevents subsequent send/download authorization; readiness is not an access grant.

Allow an empty text body only with attachments. Keep existing text limits. Add default
empty `attachment_ids` to send input and default empty attachment metadata to old
stored message/event decoding. Same-ID retries compare body and the ordered attachment
IDs, returning the original message for an exact retry and conflict otherwise.
Message, associations, event and realtime outbox writes remain atomic. History,
summaries, replay and idempotent replies hydrate the same stable metadata shape.
Do not persist signed URLs in messages, events, cache records or logs.

## Limits, leases and cleanup

Reserve a configurable 1 GiB per-user storage allowance by default, counting ready
objects and reservations for every unexpired staging attempt, including superseded
attempts. Allow at most ten pending uploads per user. Aborted, expired or failed
uploads release their reservation once after safe cleanup, accounting for all
outstanding PUT deadlines. These limits protect the
single-node product and are operator-adjustable; they are not a capacity benchmark.

Image processing accepts dimensions no larger than 8192 on either axis and at most
20 million pixels, with a best-effort 128 MiB decoder allocation limit and at most
two blocking processors. The image crate's allocation hint is not a hard process
memory guarantee; strict pixel/dimension limits and bounded concurrency also apply.
Thumbnails fit within 512 by 512 pixels. Keep a decode permit owned by the blocking
task until it exits, even when its caller times out or cancels. Bound SDK operations
and lease renewal; worker shutdown stops new claims and drains bounded work.

Media jobs use a separate leased table/loop from realtime outbox delivery. Slow object
storage and image processing must not delay chat fanout or session disconnection.
Claim/CAS/retry/cleanup SQL belongs in the media repository. External calls happen
outside database transactions. Record candidate object attempts before creation so
crashes and stale workers leave discoverable cleanup work, not untracked objects.

Expire unfinished uploads after one hour, respecting any remaining PUT lifetime.
Retry idempotent staging/candidate deletion; explicitly treat missing objects as
already deleted. Under a database lock, recheck object references and mark a record
as deletion-pending before deleting unused permanent objects. New associations must
reject that state; deletion runs outside the transaction and remains retryable.
Retain unreferenced ready uploads and superseded avatars for 24 hours before cleanup.
Referenced message objects remain retained with the message; this increment adds
neither message deletion nor a new retention policy for sent files.

## API and user interface

Add authenticated `/api/v1/media` operations:

| Method/path | Purpose |
| --- | --- |
| POST `/media/uploads` | Allocate scoped attachment and upload grant |
| POST `/media/{id}/upload-url` | Renew an unsealed upload grant |
| POST `/media/{id}/complete` | Seal and enqueue processing idempotently |
| GET `/media/{id}` | Authorized processing state and stable metadata |
| DELETE `/media/{id}` | Cancel an unreferenced upload and schedule cleanup |
| GET `/media/{id}/download` | Authorize original/thumbnail GET for 60 seconds |

Add an authenticated profile-avatar mutation to set a ready caller-owned avatar or
clear it. Exact DTOs, errors and generated types are specified in the implementation
plan. Keep reusable object upload transport separate from feature authorization.
Only feature API adapters construct requests; chat/settings components consume their
upload state owner. Clear controllers, timers, selected files and grants on logout.

Composer supports file selection, preview, progress, cancel and retry. Sending waits
for readiness and a matching membership identity. MessageList renders stable metadata
with on-demand preview/download grants; expired visible previews renew through the
adapter and stop when hidden or unauthorized. Group removal purges media with the
conversation cache. Settings provides avatar upload/clear using the same bounded flow.
Respect keyboard/IME, mobile scroll, accessible controls and existing drafts.

## Origins and deployed configuration

Serve signed objects from a separate media origin with exact CORS for the app origin.
Development supplies a separate local media entry; HTTPS tests use a second isolated
Caddy hostname/port. Production operators configure `CHAT_MEDIA_ORIGIN` and its DNS.
No public deployment or DNS change is authorized by this implementation request.

The public proxy allows only object PUT/GET/HEAD and CORS preflight for the configured
bucket/key layout, with a 50 MiB request-body cap. Deny listing, bucket administration,
delete and unsigned access. Proxy host/path must preserve the signature. Use safe
Content-Disposition filenames without CR/LF/path components; generic content is
download-only. Private internal clients continue using the unexposed SeaweedFS endpoint.

Update Vite/build/deployed CSP with only the exact media origin and `blob:` where
local image previews require it. Reserve verified image dimensions in the timeline
and reconfirm rendered visibility after load/resize before acknowledging reads.
CORS allows only required headers/methods, never
wildcard credentialed origins. No cookie is sent to object storage. Do not log signed
query strings. Pin storage/library versions and document endpoint/bucket/bootstrap
configuration without committing actual credentials or generated local S3 policy.

## Backup and actual-infrastructure acceptance

Revise coordinated backup before advertising attachment recovery: block new app writes,
pause all API/worker/media/background writers, drain in-flight work, wait out upload
grants or block public PUT, then capture a common database/filer/volume recovery point.
Always resume through cleanup/finally. Restore checks both relational associations
and exact attachment bytes after login; phase-one backups remain clearly identified.

Acceptance uses actual SeaweedFS signed PUT/GET, browser CORS/progress, mixed-text/file
idempotency, forged MIME/size, oversized/decompression input, completion retries,
staging overwrite after readiness, stale-lease candidate publication, cleanup races,
silent storage with live realtime delivery, removal/rejoin download denial, avatar
replacement/visibility, desktop/mobile media rendering, separate-origin HTTPS/CSP,
and joint backup/restore of original files and thumbnails. No mock S3 pass substitutes
for the real path. Run full integration and rebuilt deployment verification.

Official implementation references:

- [AWS Rust SDK endpoint configuration](https://docs.aws.amazon.com/sdk-for-rust/latest/dg/endpoints.html)
- [AWS Rust SDK presigned request requirements](https://docs.aws.amazon.com/sdk-for-rust/latest/dg/presigned-urls.html)
- [Image decode limits](https://docs.rs/image/latest/image/struct.Limits.html)
- [SeaweedFS bucket CORS](https://github.com/seaweedfs/seaweedfs/wiki/S3-CORS)
