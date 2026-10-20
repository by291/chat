# Product readiness acceptance

Execution base: `b472c6f` on the user's current `main` checkout. The approved account,
PWA/push and reaction plans were implemented without a worktree, then committed
in three feature groups after the user authorized local commits on 2026-10-15.
Local implementation and integrated verification completed on 2026-10-15
(Asia/Shanghai). This record distinguishes that evidence from external acceptance
that still requires operator inputs and publication authorization.

## Local evidence

| Scope                           | Observed result                                                                                                                                                                                                      |
| ------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Account password/email/recovery | Real HTTP/SQL, captured-mail browser flows, migration and protected-key restore passed                                                                                                                               |
| PWA cache/update                | Two real production builds passed offline/privacy, paused upload/draft, cancel and confirmed activation checks                                                                                                       |
| Web Push backend                | Eight binding and eleven delivery tests passed, with actual TLS/VAPID/decryption, leases, retries and authorization                                                                                                  |
| Push native recovery            | Two independent Node receiver/worker restart tests passed                                                                                                                                                            |
| Push browser                    | All three production-preview cases passed with pinned full Chromium: delayed first registration, same-session SPA disposal, encrypted delivery, actual worker presentation, duplicate tags and logout/account rebind |
| Reactions backend               | Eight HTTP/SQL regressions and populated migration 14 upgrade passed; existing message/group/read targets passed                                                                                                     |
| Reactions browser               | Both browser cases passed, including multiple devices, offline/reload, delayed acknowledgement, keyboard/mobile, withdrawal and group removal/rejoin                                                                 |
| Integrated revision             | `node scripts/check.mjs --integration` exited 0: architecture 26, Rust 160, frontend units 123, SDK/recovery 11, browsers 38; no failures                                                                            |

The combined gate also passed Rust formatting/Clippy, Svelte and service-worker
types, formatting, generated API consistency and the static build. Svelte reported
zero errors and warnings. `pnpm -C web api:generate` and `api:check` succeeded before
the gate. A separate focused push browser run passed 3/3; review/fixture units passed
16/16. After removing the completed handoff, all 87 local Markdown link targets
across 35 final documents resolved.

Environment: macOS 27.0.1 arm64, Rust 1.99.0, Node 26.11.1, pnpm 12.11.2,
Docker 29.8.2 and Playwright 1.64.0. Push used its pinned full Chromium
156.0.8078.4 (revision 1248), without the earlier Windows Chrome override.
The remaining browser cases used the corresponding pinned headless shell.
Expected revocation disconnects, terminal color notices and Node's experimental
localStorage notice appear in the successful log; they are not failed checks.

The verified implementation revision is
`cc76dc8607cfa5f6f3e82906b39de7a9d1087791`. Its runtime source/configuration
manifest contains 352 files, with SHA256
`2490dcf2b77aca227c6b1ae131783654e0ccfaf0eda5630a0f7a4d6881a05e44`.
It includes Cargo files, Rust, migrations, scripts, infrastructure, tests and `web/`
Git source, excluding ignored runtime/secret files. Source hashes remained unchanged
through the gate; subsequent edits only refresh documentation and ignored ledgers.
All 352 file hashes also match that committed revision. The pre-commit Git diff
identity remains in the ledgers as historical verification evidence.

Evidence is retained under ignored
`.superpowers/sdd/2026-10-14-product-readiness-execution/`:
`resume-final-integration.log` ends with `CHECK_EXIT_CODE=0`,
`resume-source-manifest.sha256` identifies tested files, and focused RED/GREEN logs
record the fixture regression. Owned check containers, recovery volumes and the
infrastructure lock were released; protected fixture files/logs remain local.

## Local commits

| Commit    | Scope                                                                                   |
| --------- | --------------------------------------------------------------------------------------- |
| `96d4941` | Password changes, verified mailbox enrollment, encrypted durable SMTP and recovery      |
| `427a068` | PWA assets/updates and session-owned durable background push                            |
| `cc76dc8` | Reactions, replay projections, repeatable-read snapshots and combined staging scenarios |

Shared wiring and contracts were split by feature. Each intermediate contract was
regenerated from its staged Rust snapshot. The account snapshot passed an all-target,
all-feature Rust check and the frontend scope with 91 units; the PWA/push snapshot
passed the same checks with 117 units and the worker build. The final source passed
Rust/contract checks and 44 focused frontend tests before the reaction commit.
The complete integration gate's source manifest matches the committed implementation,
so unchanged integration scenarios were not repeated just to create Git commits.

The final documentation commit records the source SHA and pending external checks.
The user requested removal of `HANDOFF.md`; lasting acceptance remains here.
No push, PR creation or deployment was authorized or performed.

## Review follow-up

The prior fresh whole-change review found three Important browser issues and no
Critical or Minor findings. All three fixes now have passing browser evidence in
the combined gate: delayed password responses preserve a newer login cookie,
initial worker registration can finish after authenticated startup, and ordinary
same-session SPA disposal preserves consent. Explicit disable/logout cleanup and
account rebinding also pass their state/browser checks. The completed review was
not repeated solely to re-inspect those fixes.

Continuation found a fresh-checkout fixture defect: the push provider started
before middleware created `.local/tests`. An isolated real-filesystem regression
failed with `ENOENT`, then passed after the provider initializes its parent.
It also checks that cleanup preserves another fixture's file. The earlier
fixed-width P-256 scalar regression remains green, and the operator key-generation
example now uses the same 32-byte representation.

## Execution rulings

The detailed plan ledgers preserve their original decisions and superseding rulings.
Material implementation choices are:

- Keep execution on current `main`, with no worktree. Grouped local commits were separately authorized after verification; push/PR/deployment remain separate actions.
- Keep anonymous recovery under cohesive `auth/recovery.rs`, separate from authenticated enrollment/password security.
- Keep fragment tokens only in browser memory and use an inline login flow for verification; GET/scanners never consume them.
- Use one final integrated gate across the focused plans rather than rerunning an unchanged full stack after every task.
- Begin repeatable-read snapshots with `pool.begin_with` rather than setting isolation after a cached statement; the real seek-history failure was reproduced and the browser target passed after correction.
- Add synchronous asset-cache `handles` so private/unlisted fetches bypass `respondWith` entirely.
- Use scoped owned production preview output and the project's pinned dependencies; development remains the default fixture mode.
- Replace the original OpenSSL-backed `web-push` candidate with pinned pure-Rust `web-push-native` 0.5.0 after Windows compilation, encrypted/signature round trip and RFC 8291 fixture verification. The temporary native build tools were removed.
- Separate push startup configuration from network transport and durable queue SQL according to responsibility, preserving public imports/contracts.
- Permit only standard HTTPS provider origins on port 443 because the selected builder's VAPID audience omits nonstandard ports. Test TLS pins a loopback socket while retaining the original signed origin.
- Use the existing `/app/chats/:id` route and a stable event tag. Existing windows navigate through the SPA to preserve drafts.
- Use full Chromium for notification tests, with an explicit local Chrome/Edge override when Windows' pinned binary has a side-by-side runtime failure. OS/external-provider delivery remains separate.
- Indicate body edits using `edited_at`; reaction projection revisions must not imply that a message was edited.
- Preserve the conversation ordering timestamp for reaction events while keeping existing event kinds' activity behavior.
- Record SQLSTATE and typed database error categories without SQL/parameters/row values, improving diagnosis while preserving public errors and secret redaction.
- Keep successful password changes cookie-neutral so delayed responses cannot delete a newer shared-browser login; durable revocation rejects the old cookie.
- Bound first worker readiness to fifteen seconds and cancel it with its authenticated owner.
- Preserve device consent across ordinary layout disposal; explicit disable, logout and identity replacement retain their cleanup boundary.
- Initialize the push fixture root before its own temporary directory, independently of middleware startup. Preserve sibling fixture files during cleanup.
- Correct the account design table to the implemented generated `/auth/password/recovery` and `/auth/password/reset` paths; API behavior is unchanged.

## External pending checks

| Check                                 | Status and required input                                                                                               |
| ------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| HTTPS staging                         | Pending a configured deployment origin, separate media origin, deployment/source identity and disposable public-signup accounts   |
| Real mailbox arrival                  | Pending an operator-controlled staging SMTP configuration and consenting mailbox; SMTP acceptance alone is insufficient |
| External Web Push/desktop OS delivery | Pending protected VAPID/provider configuration and an available consenting desktop/device observation                   |
| iPhone/iPad push presentation         | Explicitly deferred by the user; headless WebKit or emulation does not replace the installed-app/user-gesture/OS test   |
| Hosted CI for this implementation     | Pending an explicitly requested push/PR; previous hosted results do not cover the unpublished implementation            |

No production provisioning, deployment, real-user mail, external push, Git push or
PR creation was performed. Local feature commits were explicitly authorized. Tests hold their own databases, containers, processes, TLS endpoints
and fixture files. A one-off second-device login returned 503 during expanded browser
validation; safe diagnostics were added, and the same frozen-source scenario passed.
That transient did not recur during this successful combined run. Its original cause
remains unknown; no speculative functional fix is claimed.
