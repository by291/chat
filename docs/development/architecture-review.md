# Architecture and readability review

This review covers the handwritten Rust backend and Svelte/TypeScript frontend,
including transport, state owners, UI primitives and styles. The baseline is
`515f941`; the maintenance preserves application contracts and intended product
behavior. The lifetime defects corrected during verification are recorded below.

## Assessment

The modular monolith and feature-based frontend are appropriate for the current
single-host application. HTTP adapters delegate to concrete application operations;
repositories own SQL. Frontend feature adapters own HTTP, the query cache owns
server records, and authenticated state owners fence requests across logout and
membership changes. Generic repository traits, extra crates and service containers
would not resolve a demonstrated problem here.

The main concentration was media persistence: reservation, processing, references,
retirement and reconciliation shared one repository, which also made transaction
decisions. Conversation cache projections and group mutation reconciliation were
similarly embedded in larger coordinators/components. These responsibilities have
been separated, while tightly coupled stream generations and applied cursors remain
together. The device list now has a settings owner backed by the authenticated
query cache; components retain presentation and UI input state. See the current
[architecture map](../architecture.md) for reading paths.

## Readability standard

[AGENTS.md](../../AGENTS.md) defines the project's concrete policy. It follows
[Rust naming](https://rust-lang.github.io/api-guidelines/naming.html) and
[documentation guidance](https://rust-lang.github.io/api-guidelines/documentation.html),
and [SvelteKit structure](https://svelte.dev/docs/kit/project-structure),
[effects](https://svelte.dev/docs/svelte/$effect) and
[state ownership](https://svelte.dev/docs/kit/state-management).
The feature/application/repository layers are project decisions rather than a
language-mandated architecture.

The maintenance makes logical paragraphs visible: validation, authorization,
preparation, side effects and result publication have phase separation. SQL clauses,
bindings, Svelte control blocks and adjacent elements are expanded. Descriptive
records and helper names replace complex nested transforms where they identify a
real responsibility. Comments explain transaction order, request generations,
lease loss, retryability, DOM visibility and deliberate error suppression.

Large cohesive DTO, style, stream and fixture modules are retained when splitting
them would scatter one invariant. Function/file lengths remain review prompts.
Formatter success supplements manual review; it does not establish readability.

## Remaining design considerations

- The authenticated chat coordinator currently composes session-scoped media used
  by both chat and avatar settings. Moving that composition to an application owner
  could reduce settings-to-chat coupling, but needs a separate lifetime design.
- Profile/avatar presentation currently resides in settings and is reused by chat.
  A profiles feature may be useful alongside that application composition change.

The source audit also identified pre-existing lifecycle follow-ups: the root theme
initializer's disposer is unused, profile-save publication has a weaker identity
fence than the account-security forms, and start-conversation navigation lacks a
coordinator-lifetime check after its request. These need behavior regressions and
focused fixes; they are not silently changed during this maintenance.

## Verification scope

The source-level HTTP guard now includes newly introduced Rust feature directories
automatically; notifications cannot bypass it. Its regression was observed failing
before the guard change and passing afterward. The guard remains a supplement to
compiler/behavior checks; frontend aliases and dynamic transport access were also
reviewed manually.

Integration verification exposed an outdated test injection seam after extracting
presence: worker fixtures changed the limiter rather than the presence adapter.
The corrected fixtures inject the concrete dependency and require the silent
listener to accept a connection before judging deadline/cancellation behavior.
All three focused real worker regressions pass.

The final architecture review also identified unsafe automatic expiry of manually
owned canonical cache records. Cache notifications do not register query observers;
automatic five-minute collection could remove messages while their applied replay
cursor remained. Authenticated owners now retain their records until explicit
membership/reset/teardown cleanup. Virtual-clock regressions exercise record
survival and cleanup, rather than asserting a configuration literal. Settings
teardown regressions also reject late list/revoke responses after cache clearing.

The browser gate exposed another pre-existing lifetime defect: interrupted database
transaction startup could leave an untracked transaction on a pooled connection.
A real database regression reproduced a 503 from the conversation snapshot after
reusing that same connection. The database adapter now bounds release cleanup and
discards connections when cleanup fails. Focused probes also check normal commit
and rollback behavior; no application transaction is committed by pool recycling.

The complete normal and integration gate is required for this maintenance because
transactions, replay orchestration and authenticated state composition were touched.
On 2026-10-15, `node scripts/check.mjs --integration` exited 0 on the final reviewed
source. Architecture checks passed 27/27, Rust tests 162/162, frontend units 127/127,
SDK/recovery integration 11/11, and Chromium scenarios 38/38. The previously failing
older-history scenario passed, including its scroll-anchor and Unicode assertions.
Rust lint/format, Svelte and service-worker types, formatting, generated API checks
and the static build also passed. Svelte reported zero errors and warnings; generated
schemas remained unchanged. The runtime source was checked against its frozen
manifest after the command completed, with no intervening changes.

Independent whole-diff and scoped pool-recycling reviews found no unresolved findings
within this maintenance scope. Existing deployment, mailbox and device acceptance
limitations remain in
[product readiness acceptance](../operations/product-readiness-acceptance.md).
