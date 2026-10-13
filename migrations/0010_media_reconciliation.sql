-- Keep retired logical keys discoverable after a successful deletion: remote S3
-- finalization can recreate them after its caller has timed out. Tombstones are
-- permanent identities, not reusable keys; reconciliation never releases quota.
ALTER TABLE media_upload_attempts ADD COLUMN reconciliation_attempted_at timestamptz;
ALTER TABLE media_objects ADD COLUMN reconciliation_attempted_at timestamptz;

CREATE INDEX retired_attempt_reconciliation
    ON media_upload_attempts (COALESCE(reconciliation_attempted_at, deleted_at))
    WHERE deleted_at IS NOT NULL;
CREATE INDEX retired_object_reconciliation
    ON media_objects (COALESCE(reconciliation_attempted_at, deleted_at))
    WHERE deleted_at IS NOT NULL;
