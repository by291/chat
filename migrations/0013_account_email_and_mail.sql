CREATE TABLE account_emails (
    user_id uuid PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    email_address text NOT NULL CHECK (length(email_address) BETWEEN 1 AND 320),
    canonical_address text NOT NULL UNIQUE CHECK (length(canonical_address) BETWEEN 1 AND 320),
    verified_at timestamptz NOT NULL DEFAULT clock_timestamp()
);

CREATE TABLE account_security_challenges (
    id uuid PRIMARY KEY,
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    purpose text NOT NULL CHECK (purpose IN ('email_verification', 'password_reset')),
    token_hash text NOT NULL UNIQUE CHECK (token_hash ~ '^[0-9a-f]{64}$'),
    email_address text NOT NULL CHECK (length(email_address) BETWEEN 1 AND 320),
    canonical_address text NOT NULL CHECK (length(canonical_address) BETWEEN 1 AND 320),
    credential_version bigint NOT NULL CHECK (credential_version >= 0),
    email_version bigint NOT NULL CHECK (email_version >= 0),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    expires_at timestamptz NOT NULL,
    consumed_at timestamptz,
    invalidated_at timestamptz,
    UNIQUE (id, user_id)
);

CREATE INDEX account_challenges_live
    ON account_security_challenges (user_id, purpose, expires_at)
    WHERE consumed_at IS NULL AND invalidated_at IS NULL;

CREATE UNIQUE INDEX account_pending_verification
    ON account_security_challenges (user_id)
    WHERE purpose = 'email_verification' AND consumed_at IS NULL AND invalidated_at IS NULL;

CREATE TABLE account_mail_jobs (
    id uuid PRIMARY KEY,
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    purpose text NOT NULL CHECK (purpose IN ('email_verification', 'password_reset', 'email_changed')),
    challenge_id uuid,
    encrypted_payload bytea,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    expires_at timestamptz NOT NULL,
    not_before timestamptz NOT NULL DEFAULT clock_timestamp(),
    attempts integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    lease_token uuid,
    lease_until timestamptz,
    status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'delivered', 'failed', 'expired', 'cancelled')),
    finished_at timestamptz,
    failure_category text CHECK (failure_category ~ '^[a-z_]{1,64}$'),
    FOREIGN KEY (challenge_id, user_id) REFERENCES account_security_challenges(id, user_id) ON DELETE CASCADE,
    CHECK ((purpose = 'email_changed' AND challenge_id IS NULL) OR (purpose != 'email_changed' AND challenge_id IS NOT NULL)),
    CHECK ((status = 'pending') = (encrypted_payload IS NOT NULL)),
    CHECK (encrypted_payload IS NULL OR octet_length(encrypted_payload) BETWEEN 29 AND 16413),
    CHECK ((lease_token IS NULL) = (lease_until IS NULL))
);

CREATE INDEX account_mail_due
    ON account_mail_jobs (not_before, created_at)
    WHERE status = 'pending';
