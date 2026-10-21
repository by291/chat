# Engineering guidelines

These rules apply to all handwritten code, tests, and documentation in this repository.
Use English for identifiers, comments, commit messages, and documentation. Product UI
copy is a separate concern; preserve its existing localization during refactors.

## Project and scope

- Keep the Rust package at the repository root and the SvelteKit application in `web/`.
- Run frontend installs, development servers and direct CLI commands with `web/`
  as the working directory. Use the package manager pinned in `web/package.json`
  (`corepack pnpm` when the global version differs). Never create root
  `node_modules`, `.svelte-kit`, frontend package manifests or frontend lockfiles.
  Root `node scripts/check.mjs` is supported; it sets the frontend working directory.
- Read [the architecture map](docs/architecture.md) before changing ownership.
- Preserve API paths, JSON fields, error codes, database semantics, and UI behavior
  unless the task explicitly changes them. Refactoring does not authorize a migration.
- Never edit generated `web/openapi.json` or `web/src/lib/api-client/schema.d.ts`
  manually. Regenerate them from Rust with `pnpm -C web api:generate`.
- Do not commit, push, publish, or modify unrelated data unless explicitly requested.
- Keep one root `compose.yaml` for development and test infrastructure. Isolate tests
  with project names, ports and owned data, rather than additional Compose files.
  Document production service requirements without requiring a deployment platform.

## Directory and responsibility design

- Group cohesive business workflows before adding more files to a flat feature
  directory. Keep related adapters, operations, persistence and presentation near
  their owner: for example, `src/conversations/messages/` and
  `web/src/lib/features/chat/messages/`.
- Keep feature roots focused on entry points, composition and genuinely shared
  contracts. Small cohesive features may stay shallow; do not create a directory
  for every function or empty layers just to make the tree look organized.
- A move must clarify ownership. Split files when they mix responsibilities, and
  update imports, tests, comments and the architecture map together. Do not merely
  hide a flat layout behind Rust `#[path]` attributes or forwarding-only folders.
- Preserve existing public Rust paths with explicit compatibility re-exports when
  consumers need them. Internal code imports the actual owner; compatibility
  facades do not acquire business logic.
- Reuse maintained libraries for general infrastructure when they fit. Keep
  transaction, lock, lease, replay and sequence invariants explicit; do not replace
  them with a generic framework just to shorten application code.

## Readability and logical paragraphs

Code is read more often than it is written. A reader should be able to see the
workflow without decoding a dense block of statements.

- Separate imports, type declarations, constants, and function definitions with a
  blank line. Group standard-library, third-party, and project imports consistently.
- Inside a function, put one blank line between logical phases: validate input,
  authorize, prepare data, perform side effects, and construct the result.
- Keep related statements together. Do not insert a blank line after every statement
  or add decorative numbered banners that duplicate the code.
- Use descriptive names such as `conversation_id`, `session_token`, `snapshotSequence`,
  and `selectedConversation`. Short iterator names are fine only when their meaning
  is immediately visible. Avoid unexplained `r`, `n`, `p`, and `data` in workflows.
- Give complex conditions and transformations a name or a focused helper. Prefer
  early returns to deeply nested branches; use braces for multiline branches.
- Treat roughly 60–80 lines per function and 300–400 lines per workflow module as
  review prompts, not arbitrary limits. Split when responsibilities differ, not
  merely to satisfy a line count. Generated schemas and cohesive styles are exempt.
- Put SQL clauses and parameter bindings on separate lines. Keep Svelte control
  blocks and sibling elements on separate lines. Do not compress logic into macros,
  strings, chained assignments, or adjacent template branches.

## Comments and documentation

- Write comments that explain **why**, an invariant, a failure mode, or a non-obvious
  trade-off. Do not restate an obvious assignment or narrate every line.
- Document each responsibility-bearing Rust module with `//!`. Document public
  application entry points with `///`, including important error conditions and
  transaction or cancellation requirements. Use JSDoc for TypeScript application
  boundaries and state owners when their lifecycle is not obvious.
- Keep comments beside the code they explain and update them in the same change.
- Explain concurrency-sensitive code: lock order, idempotency, transaction ownership,
  continuous sequence advancement, request-generation fences, DOM visibility,
  worker leases, cancellation, and dependency timeouts.
- Explain deliberate suppression of an error. Expected ephemeral failures may be
  ignored only when expiry/retry behavior is clear. Never silently swallow durable
  data failures or bypass authorization to make an operation succeed.
- Examples should illustrate the actual project and compile when presented as a
  complete example. Label fragments clearly; do not add filler comments to meet a quota.
- Update architecture and operations documentation when ownership or commands change.

## Rust

### Module boundaries

- `bin/` contains startup and CLI adapters: parse configuration, wire dependencies,
  call application operations, and manage shutdown. Business SQL does not belong here.
- `http.rs` extracts inputs and authenticated actors, calls a service, and maps its
  result to an HTTP response. It must not query PostgreSQL directly.
- `service.rs` and cohesive workflow modules own validation, authorization, operation
  ordering, and transactions. They use repository operations rather than embedding SQL.
- `repository.rs` and focused files such as `messages/reactions_repository.rs` own
  SQL, row mappings, and database locking. Repository functions
  accept an existing transaction when the caller must compose atomic operations.
  They must not commit that caller-owned transaction or depend on HTTP handlers.
- `model.rs` owns a feature's DTOs. Shared IDs, authenticated actor, and exact sequence
  representation live under `types/`; `types` is also the explicit compatibility
  facade for existing consumers. Keep internal imports explicit instead of glob imports.
- `infrastructure/` owns configuration and external-service adapters. Do not create
  generic repository traits, service containers, or additional crates for a single
  concrete implementation without a demonstrated need.

### Language and failure handling

- Use Rust conventions: `snake_case` functions/modules, `UpperCamelCase` types, and
  `SCREAMING_SNAKE_CASE` constants. Prefer typed records over opaque workflow tuples.
- Borrow inputs when ownership is unnecessary. Clone only when a returned value,
  task boundary, or cache needs an independent owner; do not clone to conceal lifetimes.
- Return `Result` and propagate failures with `?`. `unwrap` is reserved for tests;
  a production `expect` needs a clear, documented programmer invariant.
- Keep secrets out of logs, errors, command-line arguments, and public DTOs. Preserve
  HttpOnly cookie authentication, Origin validation, and private internal credentials.
- Run blocking password work through the bounded password service. Bound external
  calls and make long-running loops cancellation-aware.
- Cancellation must cover database waits as well as network calls. Keep writes
  rollbackable when cancellation must discard them; document commit ambiguity.
  Treat a lost lease separately from a durable database failure. A missed final
  lease CAS must not commit dependent effects that require that lease.
- Keep message/event/outbox writes atomic, invite consumption atomic, and read
  receipts monotonic. Sequence values remain canonical decimal strings across JSON.
- Share typed persisted event payloads between producers and consumers, preserving
  existing tags, fields and defaults for older queued records.
- Use raw multiline strings for nontrivial SQL and Lua; never interpolate user input
  into SQL. Prefer local named constants for limits and deadlines over unexplained numbers.

## Svelte and TypeScript

- Keep `routes/` focused on routing and composition. Put business workflows under
  `lib/features/`, reusable transport under `lib/api-client/`, and shared styling under
  `lib/styles/`. Shared modules must not import features or routes.
- Reuse shadcn-svelte primitives from `lib/components/ui/` for common controls.
  Keep feature state, API calls and timers out of this presentational layer. Add
  components with the pinned CLI and `web/components.json`; review local adaptations
  before overwriting component source. Use the existing `#lib` import aliases.
- Prefer component variants and theme tokens over new global button/input/dialog
  selectors. Form buttons must declare `type="submit"` explicitly; UI components
  expose their native element through `bind:ref`, not `bind:this`.
- Feature API adapters are the only place to invoke typed HTTP methods. Components
  call their feature adapter/state owner; they must not construct API URLs or own
  socket, replay, or cache-consistency rules.
- Raw `fetch` calls belong in a feature API adapter or shared `api-client` transport.
  This also applies to browser-global calls and inline Svelte template handlers.
  Source guards check direct/static calls; review aliases and dynamic access manually.
- Give state one owner. The query cache holds authoritative server records; runes
  hold UI state such as selection, drafts, pending submissions, and connection status.
  Do not mirror the same mutable server records in a second store.
- Keep mutable synchronization internals private. Expose observations and explicit
  operations instead of letting callers change stream cursors or generation flags.
  Coalesced requests must share the promise including its error handling and cleanup,
  so every caller observes the same outcome.
- Use `$derived` for pure computed values. Use `$effect` for actual side effects,
  with explicit dependencies and cleanup. Explain `untrack` where it prevents an
  accidental subscription or lifecycle restart.
- Keep session lifetime separate from profile object identity. Revalidation must
  not discard drafts or recreate the chat state for the same authenticated user.
- Keep replay/history logic separate from ephemeral typing/presence. Centralize
  BigInt sequence comparisons, generation guards, aborts, timers, and teardown.
- A notification announces work; it does not advance the applied cursor. Only
  contiguous persisted events advance it. Read acknowledgements require both
  contiguous synchronization and a confirmed rendered/visible cursor.
- Scope contexts to the authenticated application tree. Browser globals belong in
  browser-only lifecycles; this SPA currently disables SSR. Revisit per-request state
  ownership before enabling SSR. Clear session-owned caches and timers on logout.
- The workspace owns authenticated feature startup and whole-session teardown.
  Stop feature owners before clearing the shared cache. Feature disposal removes
  only its own queries, timers and subscriptions, never another feature's records.
- Use typed props, stable keys, semantic elements, associated labels, and keyboard
  behavior. Respect IME composition and the existing mobile navigation/scroll behavior.
- Put imports, props, local state, derived values, handlers, and lifecycle effects in
  a consistent order, with blank lines between those groups. Keep markup readable
  after formatting. Use explicit spacing/CSS for inline layout rather than fragile
  indentation-dependent whitespace.
- Colocate feature styles, keep shared design tokens/layout styles separate, and
  preserve the documented CSS import order when it affects the cascade.

## Verification and review

- For code changes, run `node scripts/check.mjs` for format, lint, boundary checks,
  Rust tests, frontend types, generated API checks, unit tests, and the static build.
- Run `node scripts/check.mjs --integration` after changes to auth, message
  transactions, replay, realtime transport, session lifetime, or production wiring.
- Validate production wiring with `pnpm -C web test:deployment`
  against an explicitly configured staging deployment
  when the deployed application changes. This command does not provision a stack;
  see `docs/operations/deployment.md` for HTTPS origins and staging invitations.
- Keep public tooling in `scripts/` small and cross-platform. Use Node built-ins
  for shared commands; keep fixture lifecycle helpers beside integration tests.
  Prefer foreground service commands over custom background launchers and PID files.
- Background verification commands must suppress new windows. Use `windowsHide`
  without `detached` on Windows; reserve detached process groups for Unix signal
  isolation. Windows detached mode can create a separate console despite hiding flags.
- For refactoring, preserve existing behavior tests and add a regression only for
  a newly discovered bug or a meaningful architectural invariant. Do not write tests
  that merely repeat implementation details or assert an arbitrary file size.
- Tests should read as arrange, act, and assert, separated by blank lines. Name the
  behavior and failure condition. Isolate test databases/containers, wait for explicit
  state with a deadline, and clean up only resources owned by the test.
- Only one verification process may own the shared test infrastructure at a time.
  Wait for its cleanup before starting another infrastructure suite; do not remove
  an active ownership lock or tear down another run's resources.
- Before completion, review the architecture map, contracts, comments, documentation,
  and actual command output. A passing formatter alone does not establish readability.
- Report which checks ran and their outcomes. Distinguish source review, unit tests,
  integration tests and deployed validation. A focused rerun can resolve a known
  failure, but must not be described as a fresh successful full-suite invocation.
- For documentation-only changes, verify the diff, referenced paths and commands;
  do not start database/browser suites unless the change requires runtime evidence.

## Git delivery

- Inspect the branch, worktree and staged changes before modifying Git state.
  Preserve unrelated edits and never include generated caches or test artifacts.
- When asked to group commits, split by responsibility: workflow refactors,
  reliability fixes with their regressions, tooling and documentation. Keep imports
  with their moves and dependent changes together; use partial staging for shared
  files rather than mixing unrelated work.
- Local commit, local merge and remote push are separate actions. A request to merge
  locally means `git merge --ff-only` into the requested branch; it does not authorize
  a push. Honor explicit authorization already given in the current conversation.
- After splitting commits or merging, verify that the final tree matches the intended
  reviewed content and report any remaining changes. Git-only operations do not
  require repeating unchanged runtime tests.

## Maintaining these instructions

Keep rules concrete and specific to this repository: name the owner, path, command
or invariant. Update an existing rule instead of appending a duplicate. Put detailed
design rationale in `docs/architecture.md` and transient execution records in task
documentation, not here. Recheck examples and commands when the architecture changes.

## Reference material

The structure above is a project policy, not a claim that one architecture is
mandated by a language. Use the official references to understand the underlying tools:

- [Rust API Guidelines: naming](https://rust-lang.github.io/api-guidelines/naming.html)
- [Rust API Guidelines: documentation](https://rust-lang.github.io/api-guidelines/documentation.html)
- [The Rust Book: comments](https://doc.rust-lang.org/book/ch03-04-comments.html)
- [SvelteKit project structure](https://svelte.dev/docs/kit/project-structure)
- [Svelte effects](https://svelte.dev/docs/svelte/$effect)
- [SvelteKit state management](https://svelte.dev/docs/kit/state-management)
- [AGENTS.md format and authoring guidance](https://agents.md/)
