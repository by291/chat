# Backup and recovery

Use the deployment's database/storage snapshot tools and process manager. This
repository provides an automated recovery test; it does not provide a production
backup or restore CLI. Production does not require Compose.

## Establish a consistent recovery point

1. Close public mutation and media PUT ingress. Existing signed upload grants can
   remain valid until expiry, so block transfers at the media proxy as well.
2. Gracefully stop API and worker writers. Allow up to 90 seconds for in-flight
   transactions and storage operations to drain. A failed drain invalidates the
   recovery point; do not label that snapshot consistent.
3. Create a PostgreSQL custom-format dump or equivalent provider snapshot while
   application writers remain stopped.
4. Stop SeaweedFS and snapshot its master, filer metadata and volume files together,
   or use a storage-provider mechanism with equivalent consistency guarantees.
   Copying live volume files independently does not establish attachment consistency.
5. Store the matching application/S3 configuration, versions and a SHA256 manifest
   with the database and storage snapshots. Record the shutdown/snapshot times and
   whether both writers and PUT ingress were quiesced.
6. Restart storage and dependencies, then API and worker. Verify readiness before
   reopening mutation/media ingress.

Protect backups as private account and message data. Configuration contains secrets;
restrict access and encrypt off-site copies. Follow a retention policy appropriate
to the deployment, and measure recovery time with regular isolated drills.

## Restore into an isolated environment

1. Verify the manifest and checksums before restoring. Use an empty target with its
   own database, storage volumes, service addresses and credentials. Never restore
   over a live instance to perform a drill.
2. Restore PostgreSQL and the corresponding SeaweedFS snapshot. Use the matching
   application/schema version; migrations run at API startup. Check compatibility
   before letting a newer binary apply migrations.
3. Configure the restored API and worker to use only restored services. Keep public
   ingress closed and prevent restored sessions from reconnecting to live services.
4. Start API, then worker. Validate original-password login, revoked-session
   rejection, message history, attachment associations, original/thumbnail/file/avatar
   bytes, and processing of uploads queued at the recovery point.
5. Verify realtime recovery: PostgreSQL holds durable events; Valkey holds temporary
   rates, presence and transport history. Clients recover persisted events through
   REST even when Valkey history is empty.
6. Record results and timings. Reopen ingress only when performing an intentional
   cutover; dispose of an isolated drill's resources according to the host policy.

## Automated development drill

```sh
pnpm -C web test:backup
```

The test owns unique data in the independent `chat-test` infrastructure. It closes
media ingress, drains native writers, dumps PostgreSQL and archives stopped SeaweedFS
storage, then restores into a fresh `chat-restore-drill-*` Compose project. Checksums,
authentication, messages, attachments, exact object bytes and queued processing are
verified through the real API and worker. Only that drill project's volumes and the
source fixture's database/bucket are deleted.

The implementation lives in `web/tests/integration/recovery.ts` and the behavior
assertions in `backup-restore.test.ts`. Test artifacts remain in `.local/tests/` for
diagnosis. See [development](development.md) for interrupted-run cleanup and
[deployment](deployment.md) for production service requirements. A passing local
drill verifies the application procedure, not a provider's production backup policy.