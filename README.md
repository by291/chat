# Chat

A self-hosted chat application for individuals and small teams. The Rust package
lives at the repository root; the SvelteKit frontend lives in `web/`.

Phase one provides invitation-based registration, device sessions, exact username
lookup, private text conversations, durable messages, idempotent retries, reconnect
replay, read receipts, typing/presence, responsive Chinese UI, and themes. Groups,
attachments, editing/search, and PWA support belong to later phases.

The locked stack uses Rust 1.99, Axum, SQLx, PostgreSQL 18, Svelte 5, SvelteKit 3,
TypeScript, Tailwind CSS 4, TanStack Query, Bits UI, Centrifugo 6, Valkey 9,
SeaweedFS S3, and Caddy. PostgreSQL stores messages before an outbox worker publishes
realtime hints; those hints are never the sole record of a message.

Start with [development](docs/operations/development.md) to run the application.
For learning, read [AGENTS.md](AGENTS.md) and the
[architecture and reading guide](docs/architecture.md), then follow a complete
login or message operation through its HTTP, application, persistence, and UI layers.
Comments and documentation are English; product localization remains Chinese.

- [Single-host HTTPS deployment](docs/operations/deployment.md)
- [Backup and isolated restore](docs/operations/backup-restore.md)
- [Phase-one acceptance](docs/operations/phase-1-acceptance.md)
- [Implementation decisions](docs/operations/implementation-decisions.md)
- [Product design and roadmap](docs/superpowers/specs/2026-10-12-chat-design.md)
- [Phase-one implementation plan](docs/superpowers/plans/2026-10-12-phase-1-private-chat.md)
- [Architecture refactor plan](docs/superpowers/plans/2026-10-13-code-architecture-and-readability.md)
