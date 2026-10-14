# Messages, history and personal conversation preferences

## Replies, edits and withdrawal

Replies point to an authorized message in the same conversation. The preview follows
the target's current content and can open a surrounding history window when the
original message is not loaded. An unavailable earlier membership episode exposes
no preview. A withdrawn target displays a withdrawal indication.

An author can edit an active message or withdraw it for everyone. Editing preserves
the message ID, creation order and attachment selection. Withdrawal leaves a
tombstone in the timeline and prevents subsequent editing. It hides the body,
attachments and reply preview from history, search, summaries and event replay.
Historical event records remain in PostgreSQL; replay projects their message
payloads from current authorized state instead of serving their old content.
Fresh media download grants are also denied; already issued short-lived grants
retain their existing expiry and downloaded copies cannot be remotely erased.

Creation and modification have separate sequences. `created_seq` controls history
order and read receipts; `revision_seq` controls replacement of cached content.
Edits and withdrawals share the conversation lock, and their message/event/outbox
writes commit together. Clients compare revisions across HTTP replies, history,
search/context and replay. Quoted previews have independent target revisions.

A retry uses the original send body, ordered attachment IDs and reply target even
after an edit or withdrawal. It returns the current message rather than inserting
another one. Reusing the client ID for a different original submission fails.

## Search and contextual history

Conversation search uses PostgreSQL literal substring matching, including Chinese.
The query is trimmed and accepts 1–200 Unicode characters. `%` and `_` are literal
characters; matching is case-sensitive. Pages use the creation-sequence cursor and
exclude withdrawn messages. There is no separate search service or offline index.

Search results and reply navigation can open a bounded chronological window around
the selected message. This view does not establish a continuously applied latest
cursor and does not mark newer unseen messages read. Return to the latest timeline
before normal rendered/visible read acknowledgement resumes.

Every query rechecks current membership and the join boundary. Rejoining a group
does not restore access to earlier messages through search, replies or contextual
history. Pending responses from a previous conversation/session/episode are fenced.

## Personal preferences and notifications

Pin, mute and archive are per-user PostgreSQL preferences. Pinning sorts the
conversation first. Muting suppresses its new-message alerts. Archiving moves it to
the explicit archive view without losing history. Incoming messages do not reset
mute/archive. Preference changes and their targeted outbox invalidation commit
together, refreshing the same user's other devices without notifying other members.

Notifications operate while the application page is open. In-app alerts are the
fallback; sound and permission-based system notifications are optional. Request
permission through the notification control, never automatically during login.
System alerts omit message bodies. Unsupported or denied system notifications do
not prevent in-app alerts. Optional background delivery is separately configured;
see [PWA and background notifications](pwa-notifications.md).

Only fresh incoming messages applied through continuous synchronization can alert.
Initial history, missed replay backlog, the author's own messages, muted chats and
the currently focused visible conversation remain silent. Duplicate realtime hints
do not duplicate alerts. A receipt-triggered replay may apply a newer message before
its own realtime hint arrives; that exact hint can still announce the cached message
if it remains unread. Both acknowledged read cursors and refetched summaries suppress
already-read messages. Notification activity never advances the read cursor and its
session-owned timers/listeners are torn down on logout.

## Message reactions

Each account chooses one of `like`, `love`, `laugh`, `surprise`, `sad` or `celebrate`
per message; setting another switches, and removal clears it. Counts use that stable
order and expose only the current reader's `mine`, with no reactor directory.
Withdrawn messages show no reactions and reject changes. Former members' aggregate
counts remain, while rejoining never restores old message access.

Changed choices atomically advance the message projection revision and append
`message.reactions_updated` with its outbox. Creation sequence/time, original retry
metadata, conversation ordering timestamp and unread/read cursors remain unchanged.
Repeated desired state produces no new event. Shared reaction payloads contain only
the message ID; authorized replay hydrates a current reader-specific message or an
unavailable placeholder. Shared edit/withdraw payloads also omit reader selections.

All HTTP, history, summary, search/context and replay projections share revision-aware
merging. A delayed acknowledgement cannot replace newer counts, and mutation actions
do not establish rendered read progress. Reactions do not create open-page alerts or
background push intent. Install migration 15 and deploy API, worker and frontend
together for the new event; preserve the safe waiting-worker update policy.

## Deployment compatibility

Apply migration `0011_chat_experience.sql` through normal API startup and upgrade
the API and worker together: the worker must understand the new message events and
personal preference jobs. Keep a matching database/storage backup when upgrading;
confirm schema compatibility before restoring older application binaries. The
development Compose remains independent of the production process manager.
