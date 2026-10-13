# Backup and isolated restore

The local utility backs up the root Compose development infrastructure and restores
it into a fresh project using that same `compose.yaml`:

```powershell
./scripts/backup.ps1 -OutputDirectory './backups/2026-10-13'
./scripts/restore.ps1 -BackupDirectory './backups/2026-10-13' -ProjectName chat-restore-drill
```

The default source is `chat-dev` using `.env`. Launcher-owned native writers are
recognized through `.superpowers/dev/processes.json`. Use `-EnvFile`, `-ProjectName`,
`-DatabaseName` and `-ProcessStateFile` for a test stack. A manually started API/worker
must be stopped before backup; the utility cannot discover unrelated processes.
Production deployments use their host/provider backup tools with the same consistency
requirements, as described in [deployment](deployment.md).

The destination must be empty. A backup contains a PostgreSQL custom dump, a joint
SeaweedFS `/data` archive, the matching S3/environment configuration and a SHA256
manifest. Restrict access and encrypt off-site copies: they contain private data
and credentials. The utility copies the selected stack's S3 configuration, including
an overridden `SEAWEED_S3_CONFIG`, rather than development credentials by default.

Backup closes mutation and media PUT ingress, drains application writers, takes
the database dump and then stops SeaweedFS while archiving its metadata/volume files
together. Writers and storage resume in `finally`. Native writers need the launcher's
`CHAT_SHUTDOWN_FILE` marker for graceful draining; process paths and identities are
checked before shutdown. The phase-two manifest records `coordinatedWriters: true`;
older phase-one manifests do not establish attachment consistency.

Restore verifies hashes and requires a new `chat-restore-*` project, rejecting
existing containers, volumes or restore-state directories. Its independent PostgreSQL
binds to loopback 55435 (`-Port` changes it), S3 to 8338 (`-StoragePort` changes it),
Valkey to 56381 and Centrifugo to 8002. Run one drill at a time because the latter
ports are fixed. New volumes never overwrite a running instance. The random database
password and standard application variables are saved in
`backups/restore-state/<project>/restore.env`, never printed. Restored local HTTP
verification uses a non-Secure cookie; this configuration is for a drill only.

Validate original-password login, message history, rejection of revoked sessions,
original/thumbnail/file/avatar bytes, attachment associations and processing of
uploads queued at the backup point. Use the generated environment for isolated
native API/worker processes; do not reconnect restored sessions to the live services.

After inspection, stop only the named restore project, preserving its volumes:

```powershell
docker compose --project-name chat-restore-drill --env-file backups/restore-state/chat-restore-drill/restore.env down
```

Run this command in a fresh terminal without test/development environment overrides:
Compose process variables take precedence over `--env-file`. Valkey owns temporary
rates/presence/realtime history; PostgreSQL owns durable records. Clients recover
persisted events through REST after restore. Integration tests remove only their
uniquely named drill volumes; manual restore volumes remain for inspection.
