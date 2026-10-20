# Chat

A self-hosted chat application for individuals and small teams. The Rust package
lives at the repository root; the SvelteKit frontend lives in `web/`.

The application provides open registration and mutual friendships, device sessions, exact
username lookup, direct and private group conversations, durable messages,
idempotent retries, reconnect replay, read receipts, typing/presence, images/files,
profile avatars, message replies/edits/withdrawal, Chinese substring search,
personal pin/mute/archive preferences, six single-choice message reactions, open-page
notifications, PWA installation and safe updates, optional generic Web Push, a
responsive Chinese UI, and themes. Account security includes password change,
optional verified email enrollment and email recovery with independently queued mail.

The locked stack uses Rust 1.99, Axum, SQLx, PostgreSQL 18, Svelte 5, SvelteKit 3,
TypeScript, Tailwind CSS 4, TanStack Query, Bits UI, Centrifugo 6, Valkey 9,
SeaweedFS S3, and Caddy. PostgreSQL stores messages before an outbox worker publishes
realtime hints; those hints are never the sole record of a message.

Start with [development](docs/operations/development.md) to run the application.
For learning, read [AGENTS.md](AGENTS.md) and the
[architecture and reading guide](docs/architecture.md), then follow a complete
login or message operation through its HTTP, application, persistence, and UI layers.
Comments and documentation are English; product localization remains Chinese.

- [Production deployment](docs/operations/deployment.md)
- [Continuous integration](docs/operations/continuous-integration.md)
- [Private groups and media](docs/operations/groups-and-media.md)
- [Messages, search, preferences and notifications](docs/operations/chat-experience.md)
- [Account security and recovery](docs/operations/account-security.md)
- [PWA and background notifications](docs/operations/pwa-notifications.md)
- [Product readiness acceptance](docs/operations/product-readiness-acceptance.md)
- [Backup and recovery](docs/operations/backup-restore.md)
- [Phase-one acceptance](docs/operations/phase-1-acceptance.md)
- [Groups and media acceptance](docs/operations/phase-2-acceptance.md)
- [Implementation decisions](docs/operations/implementation-decisions.md)
- [Phase-two decisions and trade-offs](docs/operations/phase-2-decisions.md)
- [Product design and roadmap](docs/superpowers/specs/2026-10-12-chat-design.md)
- [Phase-one implementation plan](docs/superpowers/plans/2026-10-12-phase-1-private-chat.md)
- [Architecture refactor plan](docs/superpowers/plans/2026-10-13-code-architecture-and-readability.md)
- [Current architecture and readability review](docs/development/architecture-review.md)
