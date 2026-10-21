# Chat workspace redesign

## Approved design

Implement the user-approved Golo-inspired workspace: an integrated account and
conversation sidebar, a quiet message surface, compact sender identities, and a
desktop conversation overview. Use coordinated typography, spacing, avatars, and
controls across contacts, settings, and dialogs. Preserve list/detail navigation
on phones.

The final user refinement removes frequent conversations entirely. Desktop
sidebars use `clamp(288px, 23vw, 400px)` instead of a fixed narrow width. Clicking
the account avatar directly opens an editable profile dialog with avatar controls,
nickname editing, and logout. Contacts has only Friends and New Friends; the latter
combines pending incoming and outgoing requests with direction labels and matching
actions. Legacy request deep links continue to work.

## Constraints

Preserve API contracts, canonical query records, session ownership, replay,
rendered-read confirmation, drafts, and reading-position restoration. Keep saved
theme choices; use dark as the new default. Keep frontend tooling and generated
artifacts inside `web/`, with that directory explicitly selected for direct tools.
Do not commit or push.

## Implementation

1. Integrate navigation and account identity into workflow sidebars. Derive the
   desktop overview from existing state owners.
2. Apply shared fluid sizing and neutral dark/light tokens. Restyle messages,
   composer, contacts, settings, and dialogs without changing transport ownership.
3. Open the profile editor directly and preserve profile-save identity guards,
   avatar upload ownership, logout behavior, and focus restoration.
4. Combine request directions as a derived view of canonical pages. Preserve
   pagination, search, cancellation, deep links, and direction-specific actions.
5. Run project checks and browser regressions, inspect desktop/mobile screenshots,
   and address independent review findings.

## Verification results

- The direct-profile regression first failed because clicking the avatar opened
  a menu. It passes with immediately editable fields, save feedback, and draft
  preservation when the dialog closes.
- Full `node scripts/check.mjs` passed after the final changes: Rust formatting,
  lint/tests, real media checks, architecture boundaries, generated API checks,
  frontend types, formatting, 143 unit tests, and static build.
- All 15 targeted browser tests passed, covering auth/logout, Unicode profile
  editing, contacts navigation, drafts and reading positions, request acceptance,
  decline/cancellation, group membership, media uploads, avatar save/removal,
  combined request search/actions/reload, and responsive layout.
- Screenshots were inspected at desktop and phone sizes. Responsive assertions
  cover 320/390/768/1024/1440/1726/1920px widths and a 667x375px short viewport.
  The actual user Chrome window was inspected without changing account data.
- Independent review found an invisible autofocus target in the profile dialog.
  The hidden file input was removed from keyboard navigation; actual Chrome focus
  now lands on the visible Change Avatar button. No other material findings.
- Root-level `.svelte-kit/` and `node_modules/` remain absent; `web/node_modules/`
  is preserved. Deployment validation was not run.
