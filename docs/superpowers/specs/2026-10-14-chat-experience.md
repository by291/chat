# Chat experience and hosted CI repair

The user requested the complete next batch: repair hosted CI, message replies,
editing and withdrawal, conversation search, pinned/muted/archived conversations,
and notifications. Notifications are explicitly limited to an open page: in-app
alerts, optional sound and permission-based system notifications. No Web Push is
included. Engineering material remains English; product UI remains Chinese.

## Product behavior and consistency

- Replies reference an accessible message in the same conversation. Show a compact
  preview and allow navigation to its surrounding history, including unloaded history.
  Removed/rejoined members must never see earlier membership episodes through replies,
  search, contextual history or event replay.
- Authors can edit their own active messages and withdraw them for everyone. Edits
  preserve identity, creation sequence and attachments. Withdrawal is irreversible
  at the application level: show a tombstone, hide content/attachments and reject
  editing a tombstone. Repeated withdrawal is idempotent. Authorization is checked
  before idempotent success. Concurrent mutation must not resurrect withdrawn content.
- Each mutation and its durable event/outbox commit together under the existing
  conversation-first locking discipline. Message revision sequences fence stale
  acknowledgements, replay events, search/context results and history pages. Original
  send retry content remains immutable even after editing/withdrawal.
- Historical event replay must not re-expose content withdrawn since the event was
  written. Replies, summaries, searches and fresh attachment grants reflect current
  authorized state. Already downloaded bytes or issued short-lived grants cannot be
  remotely erased; existing grant expiry remains the boundary.
- Search supports Chinese literal substrings, bounded input, seek pagination and
  context navigation. PostgreSQL is sufficient; do not add a search service. Exclude
  tombstones and authorize every page against the current membership episode.
- Pin, mute and archive are per-user, persisted server preferences. They synchronize
  between that user's devices, never affect other members, and survive reload/logout.
  Archived conversations remain accessible in an explicit archive view; new incoming
  messages do not silently reset archive or mute. Pinned conversations sort first.
- Notify only for fresh, applied incoming messages; suppress self messages, muted
  conversations, initial history and replay backlog. The currently focused visible
  conversation is silent. Do not advance any read cursor because of a notification.
  Sound is user-controlled; request system permission only through a user gesture.
  Teardown notification state/listeners on logout. Permissions denied or unsupported
  browsers must retain usable in-app feedback. Avoid message bodies in system alerts.

## Ownership and verification

Retain the root Rust package, web/ SPA, existing shadcn controls and one compose.yaml.
Add cohesive conversation workflow/repository operations rather than generic layers.
Regenerate OpenAPI from Rust. Cover real database permissions, idempotency, concurrent
mutation, continuous replay and multi-device UI behavior, plus pure notification
policy. Run the root full integration check and review the resulting diff.

For CI, reproduce infrastructure startup, expose useful bounded failure output and
collect owned fixture diagnostics without exporting secret environment files. Repair
the observed cause rather than weakening checks. Keep jobs parallel and Windows
commands hidden/non-detached. Commits and pushes require a separate explicit request;
local validation does not establish a hosted run of unpushed changes.
