# Implementation decision record

The entries below retain phase-one rulings in execution order. This directory has
no Git repository; source and local verification evidence remain in place.

- work in current directory, no worktree and no commits — explicit user location and no Git repository — cost if wrong: can initialize Git later.
- manual ledger/task briefs — skill scripts require a Git repository and bash — cost if wrong: evidence remains local instead of Git history; preserve scratch.
- web/pnpm-workspace.yaml is pnpm 12 generated supply-chain policy only, not a multi-package workspace — retain policy rather than disable checks — cost if wrong: move policy when pnpm supports another location.
- SvelteKit 3 tsconfig extends $app/tsconfig, verified by tool warning and successful build — fixes outdated scaffold expectation — cost if wrong: one config change.
- Corepack cache differs in sandbox; run pnpm in web with escalated tool to use installed pinned version — avoid latest lookup — cost if wrong: none to product.
- event payload uses {type,data} tagged Rust enum — plan leaves payload shapes open, shared generated DTO will reflect it — cost if wrong: versioned payload migration.
- launch copies of Rust executables in integration scratch to avoid Windows target executable lock — actual runtime remains same built binary — cost if wrong: startup copy.
- launcher stdio ignored and state file read after process exit — Windows inherited handles kept captured Node exec pipe open even after PowerShell exited — cost if wrong: startup errors diagnosed from logs.
- implement Task 6 DB lease part while Task 5 SDK test runs — shared Rust proxy contract already verified; all realtime integration must pass before frontend — cost if wrong: fix integration before next phase.
- use maintained @lucide/svelte instead of deprecated lucide-svelte; protobufjs postinstall inspected (local version checks only) and explicitly allowed; runed Kit 3/openapi-typescript TS 6 peer declarations require runtime/build compatibility verification in Task 8.
- Centrifugo proxy forwarded Cookie/Origin are HTTP request headers, not JSON body headers — official v6 docs and real integration showed mismatch in plan assumption; JSON headers now ignored — cost if wrong: handler contract adjustment.
- SvelteKit 3 #lib alias must be explicitly declared in package.json imports with extension-bearing imports — official migration guide and type-check failures confirm — cost if wrong: alias config change.
- ChatSync takes authenticated userId in addition to planned options — needed to match optimistic messages by sender and client ID, preventing another sender from clearing a pending entry with the same client ID — cost if wrong: constructor option only.
- Playwright stack is test-scoped instead of worker-scoped — isolates database and real per-IP registration limits across scenarios without disabling the product limiter — cost if wrong: a few seconds startup per test.
- trust only the configured Caddy IP and accept one parsed forwarded IP — required for useful per-client production limits without trusting arbitrary XFF — cost if wrong: proxy network configuration update.

The dependency-compatibility ruling costs a package-import or pnpm-policy change
if wrong. TypeScript 6 generation, Bits UI Dialog and SvelteKit 3 passed actual
runtime/build checks.

The original fresh-context review used gpt-6-astra and found no Critical issue.
It identified two Important issues and noted identity-only foreground restoration.
Because the latter could hide saved messages until polling, it was regraded Important
and included in the same single repair pass.

- Foreground restoration is Important and fixed: approved behavior refreshes summaries
  and current messages immediately; cost if wrong: a coalesced refresh per focus/visibility event.
- Groups, media, and SeaweedFS attachment recovery remain phase two: current product is
  private text with configured storage/joint backup; cost if wrong: implement and accept
  those workflows before that phase ships.
- Editing/deletion/reactions/search/preferences/PWA/offline sending remain later phases:
  they are absent today; cost if wrong: another implementation and acceptance pass.
- HA and E2EE remain outside phase one: single-node server-readable cloud history;
  cost if wrong: redundant infrastructure or a redesigned encryption protocol.

Receipts now require contiguous application plus a rendered cursor; completion of
an earlier receipt cannot acknowledge an unrendered later message. Valkey candidate
queries and the complete worker maintenance pass have two-second deadlines and
cancellation, keeping durable jobs responsive. Foreground identity restoration is
followed by summary/replay refresh. Each regression failed before its fix and passed
afterward. See phase-one acceptance for complete verification.

The original review deferred no Minor findings. Later capabilities follow the phase
rulings above.

## Architecture refactor rulings

- Implement the explicitly requested reversible refactor inline without another
  approval round; cost if wrong: restore the preserved original-source snapshot.
- Preserve scratch and omit Git initialization/commit, since neither was requested;
  cost if wrong: evidence remains local until version control is introduced.
- Use cohesive concrete features instead of generic repository traits/containers;
  cost if wrong: introduce a specific abstraction when a second implementation needs it.
- Use English development comments/documentation and preserve localized product/API
  copy; cost if wrong: a separately reviewed localization change.
- Keep groups, attachments, PWA, HA and E2EE in their existing future scope; the
  refactor delivers the same private text product with clearer architecture.
  Cost if wrong: implement and accept the relevant phase before relying on it.
- Make no large-scale capacity claim for this single-host refactor; cost if wrong:
  representative load testing and capacity changes before broader deployment.

The fresh architecture review found no Critical or concrete runtime regression.
The application-to-HTTP dependency was fixed by moving transport extraction into
the realtime HTTP adapter; its new architecture regression failed before the fix
and passed afterward. Remaining dense component/worker/test examples were expanded.
Two Minor findings are deferred: direct/global `fetch` detection in the frontend
guard (no current component violation), and moving `LeasedJob` out of worker
orchestration to remove its small repository import cycle. The ownership guards
supplement compilation and review; they are not a complete dependency analysis.
