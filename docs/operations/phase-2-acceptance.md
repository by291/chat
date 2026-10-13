# CI, private groups and media acceptance

This records the earlier acceptance run, including its former production Compose
harness. Current local tests share the root `compose.yaml`; HTTPS checks now target
a configured staging deployment. Use the development/deployment guides for commands.

Verified locally on Windows on 2026-10-13, with real PostgreSQL, Valkey,
Centrifugo, SeaweedFS and Chromium. No commit, push or public deployment was made.

## Final commands

```powershell
./scripts/check.ps1 -IncludeIntegration
./scripts/production-test.ps1 -Prepare
pnpm -C web test:deployment
```

All exited 0 after the final review fixes. The full check includes formatting,
all-target Clippy, architecture guards, resolved production Compose configuration,
Rust tests, generated API consistency, Svelte types, frontend tests, static build,
real storage/associations, realtime SDK integration, joint recovery and browsers.

| Verification | Passed |
| --- | --- |
| Architecture guards and production Compose configuration | 27 |
| Rust base | 52 |
| Actual media and atomic associations | 29, including 5 already in the base |
| Frontend unit tests | 24 |
| Realtime authentication, delivery and ephemeral integration | 6 |
| Joint backup/restore | 1 |
| Application browser scenarios | 20 |
| Separate-origin HTTPS deployment | 2 |

This is 156 distinct cases; Rust is 76 after removing the overlap. Svelte reports
zero errors and zero warnings. Formatter, lint, contract and build checks add no
cases to that count. Workflow actionlint and actual disposable CI storage/realtime
smoke checks also passed. GitHub Actions has not run for these uncommitted changes;
inspect the hosted run after pushing before claiming hosted-runner acceptance.

## Verified behavior

Private group roles, single-use invitations, ownership transfer, removal/rejoining,
episode history boundaries, read counts, typing, reload/reconnect and mobile/IME
behavior run through real browsers. Summary authorization and message metadata
share a database snapshot. Old episode requests cannot bind uploads or acknowledge
new history.

Media acceptance covers signed PUT/GET, native upload progress, interruption/retry,
file-only sends, ordered idempotent references, exact original/download bytes,
sanitized previews/avatars, quota/lease/cleanup races and storage failure while
realtime delivery continues. Retired staging/candidate keys recreated after deletion
are discovered and deleted again without releasing quota twice.

The HTTPS build verifies secure cookies, deep links, private internal endpoints,
exact CORS, CSP, signed images and avatars, nosniff and private/no-store responses.
Application and deployment browser suites use sibling output directories to avoid
one suite deleting another's live trace artifacts.

Joint recovery quiesces writers before database/file capture, restores into an
isolated project, verifies original/thumbnail/file/avatar bytes and associations,
preserves revoked-session rejection, and processes a genuinely queued upload.
Known restored object bytes are probed with a deadline because root S3 liveness
can precede restored volume routing; persistent failure or wrong bytes still fail.

## Review and operating boundaries

A fresh whole-change review requested three corrections: production group-limit
forwarding, roster concurrency and retired-key reconciliation. All were fixed in
one wave; scoped re-review passed spec and quality. Group SQL and media markup
readability were corrected in that wave too.

See [groups and media](groups-and-media.md) for limits, permanent tombstones and
physical-storage/decoder boundaries, [backup/restore](backup-restore.md) for the
maintenance procedure, and [phase-two decisions](phase-2-decisions.md) for execution
rulings and reversal costs. Local evidence remains ignored under
`.superpowers/sdd/2026-10-13-seaweedfs-media/`: `final-check-isolated.log`,
`final-production.log`, `final-deployment-isolated.log`, `final-review.md` and
`final-fix-review.md`.
