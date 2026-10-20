# Workspace Redesign Implementation Plan

> Execute inline with superpowers:executing-plans. Preserve uncommitted work.

**Goal:** Replace inconsistent page navigation with a responsive workspace.

**Architecture:** Authenticated composition owns one navigation shell. Features
retain canonical server state; URLs and session UI context own view state.

**Tech Stack:** Svelte 5, SvelteKit, shadcn-svelte, TypeScript, Playwright.

**Spec:** `docs/superpowers/specs/2026-10-21-workspace-redesign-design.md`

## Global constraints

- Keep API paths, DTOs, authentication and replay/read semantics unchanged.
- English code/docs, Chinese UI, existing dependencies. No commits or deployment.

## Review focus

- History must work for contact details and settings categories.
- Settings return must retain chat selection, draft and reading anchor.
- Reading positions must not cross membership episodes or imply read status.
- Mobile details need top-left Back; primary lists share bottom navigation.
- Category changes preserve drafts; leaving settings discards passwords.

## Tasks

- [x] Replace scrolling-settings assertions with category visibility, direct-link,
      history and draft tests in `web/e2e/settings-navigation.spec.ts`. Add navigation
      regressions in `web/e2e/contacts-navigation.spec.ts`. Confirm initial failures.
- [x] Rebuild `routes/app/+layout.svelte`, chat route composition and shared
      `styles/app-shell.css`/`responsive.css`; extract labeled responsive navigation.
- [x] Add a settings feature page and fragment category navigation, keeping forms
      mounted within the route. Update affected browser tests to choose categories.
- [x] Make contacts category/selection URL driven. Preserve scroll and focus;
      improve rows, category controls and empty states.
- [x] Refresh chat list/header/empty states; store and restore reading anchors in
      ChatContext with membership isolation. Distinguish loading from unavailable.
- [x] Run focused browser tests, then `node scripts/check.mjs --integration`.
      Review desktop/mobile screenshots and update architecture/experience docs.

## Approved revision and execution notes

- Ruling: the final approved rail mockup supersedes the earlier top-bar proposal.
  Continue on the existing feature branch and preserve its uncommitted feature base.
  No commits, push, deployment or backend contract changes are authorized.
- Earlier regressions reproduced settings category visibility and lost reading anchors.
  Their lifecycle fixes remain; this revision adds mobile settings parent navigation.
- Implemented the shared application bar, pane geometry, Jade tokens, avatar tones,
  grouped settings/contact surfaces and composer styling. Browser verification pending.

## Final approved implementation ledger

- Baseline: earlier focused browser gate passed 4/4. Contact pagination regression
  is intentionally RED because `ContactsState.reveal` has not been implemented.
- Ruling: keep existing group creation/join, outgoing requests and avatar upload
  capabilities; the mock only removes the redundant direct-chat entry, not those
  workflows. Group actions stay in an explicitly labeled menu.
- [x] Replace the top bar with the rail/mobile bottom navigation and shared account menu.
- [x] Move own-profile viewing/editing into a centered dialog; keep settings independent.
- [x] Add local chat detail panels without route changes.
- [x] Unify searchable contacts/request lists, complete deep-link pagination and focus.
- [x] Remove direct-chat creation duplication while retaining group workflows.
- [x] Verify focused regressions, full integration gate, screenshots and final review.

- Implementation: account modal, context detail panels, rail, mobile bottom navigation,
  stable search, request direction filter, and explicit group actions are in place.
- Unit regression: canonical contact deep-link pagination changed from missing-method
  RED to 5/5 passing contacts tests. Svelte check reports 0 errors and 0 warnings.
- Initial browser results: contacts navigation, reading anchors, settings drafts/history,
  and modal keyboard/mobile bounds pass. Remaining profile cases and full gate pending.
- Screenshot inspection: desktop contacts and mobile contacts/settings geometry verified;
  aligned contacts search/filter spacing with chat's shared search geometry afterward.

## Final review and correction pass

- Fresh review found interrupted canonical pagination, a late group-join redirect,
  and an incoming badge displayed on outgoing requests.
- Pagination regression reproduced an empty deep-link result after a concurrent
  refresh; it now resumes the interrupted read. Contacts unit suite: 7/7 passing.
- Browser regressions reproduced both the outgoing count mismatch and the missing
  group-join cancellation before their fixes. Joining now passes an AbortSignal and
  checks the originating component and authenticated owner after each await.
- Count accuracy is important to the approved request-list design, so the outgoing
  badge issue is fixed rather than deferred as cosmetic. No review findings deferred.
- The first full gate passed architecture, Rust format/lint/tests, and Svelte/worker
  types. Its formatting phase encountered the newly added unformatted regression;
  formatting is corrected and a fresh full integration gate is running.
- Full gate progress: 141 frontend unit tests and 12 SDK/recovery integration tests
  pass, together with Rust checks, types, formatting, API contract and static build.
- Browser sweep exposed old test assumptions. Profile Unicode coverage now opens the
  avatar modal. Shared-cookie password-change coverage waits for the login page's
  restored `/app` destination and verifies old password fields are cleared.
- Trace evidence showed two chat scenarios clicking the empty-state Contacts link
  before the peer's conversation arrived. Their fixture now waits for the specific
  peer username and completed chat navigation. Product/auth behavior is unchanged.
- Fresh frontend verification passes all 141 unit tests, types, formatting, API
  generation checks and static build. A fresh browser batch passes all 28 selected
  scenarios, including every earlier failure and both browser review regressions.
- The original full browser sweep was interrupted after 27 passes because it had
  loaded obsolete test assumptions at startup. Its exit status is not a passing
  full-gate result; final browser coverage is reconciled by scenario across runs.
- Visually inspected real desktop/mobile screenshots for chat, contacts, requests,
  own-profile dialogs, peer detail panels and narrow settings. Saved review images
  under `.local/ui-review/workspace/`.
- Background push verification passes 3/3 after updating the remaining old test
  navigation to open the notification settings category. Real encrypted delivery,
  worker notification presentation and same-session subscription retention pass.
- Final coverage reconciliation matches all 52 browser scenarios by file and title
  across the original sweep and successful fresh batches. No failures remain
  without a passing rerun. Verification evidence: `/private/tmp/chat-final-check-green.log`,
  `/private/tmp/chat-browser-followup.log`, and
  `/private/tmp/chat-background-followup-green.log`.
- Final architecture/contract/documentation review and `git diff --check` pass.
  No commits, push, deployment or changes to unrelated working-tree data were made.

## Local delivery authorization

- The user subsequently authorized granular local commits: public registration,
  backend friendship policies, session-owned contacts state, dropdown primitives,
  and workspace UI.
  Push and deployment remain outside this authorization.
- Final visual adjustments retain a 64px rail and 360px list. The desktop logo is
  42px with 16px top spacing; navigation controls are 40px. Search, category controls
  and list content share consistent horizontal insets.
- A fresh `node scripts/check.mjs` passes Rust format/lint/tests, architecture and
  API checks, frontend types/formatting, all 141 frontend unit tests and static build.
  Earlier integration coverage includes 12 SDK/recovery tests and all 52 browser
  scenarios, with failed legacy test assumptions corrected and rerun.
- Removed two misplaced root `.svelte-kit` type files generated by an earlier
  manual sync command run from the repository root. The check runner already runs
  SvelteKit in `web/`; its proper generated cache remains ignored there.
