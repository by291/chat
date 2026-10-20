# Workspace navigation redesign

## Intent

Rebuild the authenticated interface around predictable navigation and readable
chatting. The user explicitly permits replacing the current page design.
Preserve business capabilities, API contracts, authentication and consistency
invariants. Existing uncommitted friendship work remains the implementation base.

## Approved visual direction

The user approved the final solid-color rail mockup after reviewing desktop and
mobile variants. Use a 64px application rail, a compact contextual list and a
content pane; mobile primary navigation belongs below the list.
The reference is Telegram Web A's consistent list/detail hierarchy, adapted to
our three primary workflows: https://github.com/Ajaxy/telegram-tt.

Use green accents, stable avatar tones, subtle chat texture, raised composer,
file previews and grouped profile/settings surfaces. Retain existing capabilities;
mockup sample content such as pinned announcements is not a new backend feature.

## Layout and behavior

- Desktop: a 64px rail, 360px feature sidebar and content pane.
  Align pane titles in 60px headers; place account actions at the rail bottom.
- At 700px and below: three destinations below the feature list, with the account
  menu in the list header. Conversation,
  contact and settings details replace lists, with Back at the top left. Each pane
  owns scrolling.
- Keep existing route paths. Contact category/selection become query parameters.
  Settings uses existing `#settings-*` fragments to select actual category panels.
- Primary Chat resumes the current conversation. Explicit chat
  Back opens the list. Contact Back removes selection while preserving category.
- Settings shows one category at a time. Keep panels mounted within the route to
  preserve form drafts between categories; discard password fields on leaving it.
- Own profile is accessible only through the avatar menu: a centered dialog first
  shows details, then provides editing within the same dialog. Settings contains
  security, notifications/appearance and application controls, without profile editing.
- Direct-peer and group information opens locally in a right panel (mobile bottom
  panel), preserving the current conversation and draft. Keep group operations.
- Remove redundant direct-chat creation entries; start direct chats from a friend's
  profile. Retain explicit group creation/join operations and adding friends.
- Friends and incoming requests share the same search/list/header geometry.
  Render each request as a person row; selected details contain the actual available
  request fields and actions. Preserve outgoing request cancellation. Do not invent
  a request message field the API does not provide.
- Use solid surfaces and avatar colors. Do not reproduce decorative gradients.
- Retain message anchors in authenticated UI context, keyed by conversation and
  membership episode. Restore the same message offset after remount. Only readers
  already following latest follow new messages. Never persist message content or
  read acknowledgements in this view state.
- Cold conversations show loading before unavailable. Keep cached content and all
  existing request-generation, replay and rendered-visibility fences.
- Keep notifications, group management, media and message operations accessible.

## Ownership and verification

The authenticated layout owns navigation and chat route composition. ChatContext
owns transient list/timeline positions. Contacts owns URL-driven selection;
settings owns category navigation and forms. Shared CSS owns workspace geometry.
Use existing shadcn-svelte controls and Lucide icons; no new dependencies.

Test desktop and 390/320px layouts, direct links, history, drafts, reading restoration,
dialog focus, theme and notification visibility. Run
`node scripts/check.mjs --integration` serially and inspect screenshots.
Production staging validation applies only when a configured deployment changes.
Do not commit or push.
