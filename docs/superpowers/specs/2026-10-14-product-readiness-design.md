# Product readiness design package

**Status:** Written designs approved by the user on 2026-10-14. The user confirmed the
[roadmap](../plans/2026-10-14-product-readiness-roadmap.md), selected verified email
recovery and deferred iPhone/iPad real-device push acceptance. Product implementation
has not started; these documents settle the new interfaces and consistency rules.

## Intended result

Complete assessment items 1, 2, 3 and 5 for the existing self-hosted small-team chat:
account self-service, PWA/background notification/reactions, version-specific release
verification and consistent engineering/operator documentation. Keep item 4's capacity,
extended stability and production monitoring/backup automation in its separate scope.

## Designs and delivery order

1. Release preparation and operating-document correction from roadmap milestone A.
2. [Account security and email recovery](2026-10-14-account-security-design.md),
   milestones B/C, with password change followed by email enrollment/mail and recovery.
3. [PWA and background notifications](2026-10-14-pwa-push-design.md), milestones D/E.
4. [Message reactions](2026-10-14-message-reactions-design.md), milestone F.
5. Integrated local checks, current-revision hosted/staging acceptance and documentation,
   milestone G. Document behavior with each corresponding change.

Account credential/session changes provide the revocation boundary used by push.
Mail and push are separate bounded durable loops: a provider timeout cannot delay
realtime delivery or terminate unrelated worker loops. Reactions retain the existing
conversation lock and message revision merge. PWA assets never persist authenticated
records. Each subsystem remains independently testable and reviewable.

## Release verification and operator inputs

Keep the three existing CI jobs and one root `compose.yaml`; do not provision a new
platform or add a second infrastructure definition. Correct obsolete production Compose
and editing/search instructions in the current groups/media guide. Preserve clearly
dated historical acceptance evidence.

Run `node scripts/check.mjs --integration` against the integrated source. Record exact
revision, generated-contract result and command output. A later requested push/PR
must verify all three hosted jobs for that same revision. No commit, push, PR creation
or deployment is authorized merely by these design documents.

Use an operator-provided disposable HTTPS staging application and separate media
origin with fresh invitations. Extend the existing deployment scenarios for account,
PWA and reaction contracts; mail requires a consenting test mailbox and protected SMTP
configuration. Real push delivery requires protected VAPID configuration and an
available device. None of these inputs is assumed to exist locally.

iPhone/iPad OS notification acceptance remains deferred and explicitly unverified.
It does not block local implementation or automatic tests. Provider acceptance and
automated worker tests do not imply OS presentation. Other unavailable external
acceptance checks remain separately listed until executed.

## Review focus

- Credential-change races must not let an old password, session or mail token undo a
  completed change; durable failures must not partially consume security challenges.
- Mail links must survive intended sign-in without moving tokens into logged query
  parameters; scanner GETs must not mutate credentials or consume a link.
- Device/account changes must not rebind queued pushes to another user's browser
  session or expose content through stale provider messages.
- PWA updates/cache rules must preserve ongoing work and exclude private response data.
- Reaction acknowledgements/replay must never resurrect stale counts or withdrawn
  content, leak actor-specific flags, or advance unread/read state as a new message.

Next, produce focused implementation plans with exact tasks,
file/interface mappings and verification commands, then select the execution method.
