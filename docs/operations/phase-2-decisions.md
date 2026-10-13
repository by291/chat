# Phase-two implementation decisions

These entries retain execution rulings and the cost of reversing each choice.
Group decisions precede media decisions, following the implementation sequence.
No commit, push or public deployment is part of this work.

- Keep the current checkout/main — user explicitly requested root/current-directory work and direct continuation — cost if wrong: move uncommitted work to a worktree.
- Preserve evidence and skip skill-script commit/cleanup steps — AGENTS requires explicit commit authorization; PowerShell bookkeeping records uncommitted ranges — cost if wrong: evidence remains ignored until the later requested commit.
- Use a cohesive group_repository.rs alongside the shared repository — group SQL has separate role/invitation responsibility and avoids inflating message SQL — cost if wrong: combine those persistence operations without changing contracts.
- Validate final production wiring after media rather than rebuild twice — media changes the same Compose/Caddy surface; group integration still runs first — cost if wrong: a group-only deployment defect is found at final acceptance rather than earlier.
- Keep the group HTTP adapter cohesive in group_http.rs — the shared adapter already owns direct/message/replay endpoints and should stay readable — cost if wrong: combine adapters without changing their service boundary or URL contracts.
- Removal/demotion permanently revokes outstanding issuer links — rejoining or regaining admin should not reactivate links issued under lost authority — cost if wrong: an admin issues a fresh invitation after restoring permission.
- Allow cohesive *_repository.rs persistence and *_http.rs adapter names in source guards — matches the approved file map and keeps SQL and imports enforced for media too — cost if wrong: narrow the filename policy without changing runtime modules. RED import fixture/source-guard-red.log then source-guard-green.log proves 26/26 checks.
- Keep media scheduling separate from realtime delivery — untrusted decode and storage timeouts cannot delay messages or session revocation — cost if wrong: one extra durable queue/loop to maintain, with shared documented lease discipline.
- Use explicit SDK clients and minimal features rather than environment/profile discovery — deployments require only Seaweed credentials/endpoints — cost if wrong: introduce a provider only if another storage deployment later requires it.
- Image allocation limits remain best effort — strict dimensions/pixels and two processors supplement library limits — cost if wrong: add process isolation if measured decode behavior exceeds the single-node resource envelope.
- Delegate media backend in parallel with group frontend — completed group interfaces and disjoint file ownership allow progress without conflicting edits — cost if wrong: reconcile the interfaces and repeat scoped integration verification.
- Prepare Task 3 transport/state while Task 1 compiles — the fixed brief DTOs and feature API keep files disjoint; integration still waits for Tasks 1–2 gates — cost if wrong: adjust the typed adapter after the authoritative Rust contract is generated.
- Make production bucket initialization an explicit setup-profile command — a storage outage must not block text API startup through a Compose dependency — cost if wrong: operators run one initialization/recovery command before uploads work.
- Give each browser stack a unique chat-e2e-UUID bucket and restrict CLI cleanup to that exact prefix/hex identity — prevents dropped test DBs orphaning a shared bucket — cost if wrong: one extra bootstrap/cleanup command per owned test stack.
- Retain retired staging/candidate identities and reconcile their logical keys hourly in bounded oldest-due batches — SeaweedFS background finalization can recreate a key after the original successful DELETE — cost if wrong: permanent tombstone metadata and periodic idempotent deletes; physical chunk reclamation is not promised.
