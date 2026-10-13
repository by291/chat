ALTER TABLE outbox ADD COLUMN attempt integer NOT NULL DEFAULT 0;
ALTER TABLE outbox ADD COLUMN next_attempt_at timestamptz NOT NULL DEFAULT now();
ALTER TABLE outbox ADD COLUMN lease_token uuid;
ALTER TABLE outbox ADD COLUMN lease_until timestamptz;
ALTER TABLE outbox ADD COLUMN last_error text;
CREATE INDEX outbox_pending ON outbox(next_attempt_at,created_at) WHERE completed_at IS NULL;
