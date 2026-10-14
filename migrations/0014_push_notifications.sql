ALTER TABLE sessions ADD CONSTRAINT sessions_identity UNIQUE (id, user_id);

CREATE TABLE push_subscriptions (
    id uuid PRIMARY KEY,
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    session_id uuid NOT NULL,
    generation uuid NOT NULL UNIQUE,
    endpoint text NOT NULL CHECK (octet_length(endpoint) BETWEEN 1 AND 2048),
    endpoint_hash text NOT NULL CHECK (endpoint_hash ~ '^[a-f0-9]{64}$'),
    p256dh text NOT NULL CHECK (length(p256dh) = 87),
    auth text NOT NULL CHECK (length(auth) = 22),
    enrolled_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    revoked_at timestamptz,
    FOREIGN KEY (session_id, user_id) REFERENCES sessions(id, user_id) ON DELETE CASCADE
);
CREATE UNIQUE INDEX push_active_session ON push_subscriptions(session_id) WHERE revoked_at IS NULL;
CREATE UNIQUE INDEX push_active_endpoint ON push_subscriptions(endpoint_hash) WHERE revoked_at IS NULL;
CREATE INDEX push_endpoint_history ON push_subscriptions(endpoint_hash, enrolled_at DESC);
CREATE INDEX push_account_devices ON push_subscriptions(user_id) WHERE revoked_at IS NULL;

CREATE TABLE push_activity (
    subscription_id uuid PRIMARY KEY REFERENCES push_subscriptions(id) ON DELETE CASCADE,
    conversation_id uuid REFERENCES conversations(id) ON DELETE CASCADE,
    expires_at timestamptz NOT NULL
);

CREATE TABLE push_jobs (
    id uuid PRIMARY KEY,
    event_id uuid NOT NULL REFERENCES conversation_events(event_id) ON DELETE CASCADE,
    event_seq bigint NOT NULL CHECK (event_seq > 0),
    message_id uuid NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    conversation_id uuid NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    subscription_id uuid NOT NULL REFERENCES push_subscriptions(id) ON DELETE CASCADE,
    membership_id uuid NOT NULL REFERENCES conversation_members(membership_id),
    binding_generation uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    deadline timestamptz NOT NULL,
    not_before timestamptz NOT NULL,
    attempts integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    lease_token uuid,
    lease_until timestamptz,
    status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','delivered','skipped','expired','failed')),
    finished_at timestamptz,
    failure_category text CHECK (failure_category ~ '^[a-z_]{1,64}$'),
    UNIQUE (event_id, subscription_id),
    CHECK ((lease_token IS NULL) = (lease_until IS NULL))
);
CREATE INDEX push_jobs_due ON push_jobs(not_before, created_at) WHERE status = 'pending';
