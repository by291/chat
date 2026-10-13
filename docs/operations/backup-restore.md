# Backup and isolated restore

```powershell
./scripts/backup.ps1 -OutputDirectory './backups/2026-10-13'
./scripts/restore.ps1 -BackupDirectory './backups/2026-10-13' -ProjectName chat-restore-drill
```

The default source is project `chat` using `.env.production`. Use `-EnvFile`,
`-ComposeFile`, `-ProjectName`, and `-DatabaseName` to select a specific development
or test stack. The output directory must be empty. A backup contains a PostgreSQL
custom dump, a joint SeaweedFS `/data` archive, S3/environment configuration, and SHA256
manifest. It contains private messages and credentials; restrict access and encrypt
its off-site storage.

PostgreSQL uses a consistent snapshot. SeaweedFS stops while its local master/filer
metadata and volume files are archived together, then resumes. Phase one has no attachment
writes, so it needs no cross-system write freeze. Before phase-two attachments, pause
every API/worker/background writer, finish in-flight uploads, record a common recovery
point, and then take the joint backup. Attachment recovery acceptance belongs to phase two.

Restore verifies integrity and requires a new `chat-restore-*` project. It rejects
existing containers, volumes, or restore-state directories. New PostgreSQL/SeaweedFS
volumes are created without replacing a running instance. The database validation port
binds only to 127.0.0.1:55435 by default; `-Port` changes it. A random restore password
is saved in `backups/restore-state/<project>/restore.env` and is not printed.

Validate original-password login, conversation history, continued rejection of revoked
sessions, and resumption of pending outbox jobs. Use isolated API/Valkey/Centrifugo
instances for the restored database. Do not attach restored sessions to the running
deployment.

After validation, stop that project using its restore environment and
`infra/compose/compose.restore.yaml`. Keep restore volumes for inspection; cleanup must
target only that named project. Valkey owns temporary rates/presence/realtime history;
PostgreSQL owns durable data. Clients recover persisted events through REST after restore.
