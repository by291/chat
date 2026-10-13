-- Quota rows serialize allocation, publication and cleanup. Application lock order:
-- conversation (when scoped), user quota, sorted attachments, jobs and objects.
CREATE TABLE media_quotas (
    user_id uuid PRIMARY KEY REFERENCES users(id),
    used_bytes bigint NOT NULL DEFAULT 0 CHECK (used_bytes >= 0)
);

CREATE TABLE attachments (
    id uuid PRIMARY KEY,
    owner_id uuid NOT NULL REFERENCES users(id),
    purpose text NOT NULL CHECK (purpose IN ('message', 'avatar')),
    conversation_id uuid REFERENCES conversations(id),
    membership_id uuid REFERENCES conversation_members(membership_id),
    state text NOT NULL CHECK (state IN ('uploading', 'processing', 'ready', 'failed', 'expired')),
    filename text NOT NULL,
    content_type text NOT NULL,
    size_bytes bigint NOT NULL CHECK (size_bytes BETWEEN 1 AND 52428800),
    width integer,
    height integer,
    sha256 text,
    error_code text,
    active_attempt_id uuid,
    ready_bytes bigint NOT NULL DEFAULT 0 CHECK (ready_bytes >= 0),
    deletion_pending boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CHECK ((purpose = 'message' AND conversation_id IS NOT NULL AND membership_id IS NOT NULL)
        OR (purpose = 'avatar' AND conversation_id IS NULL AND membership_id IS NULL))
);
CREATE INDEX media_pending_owner ON attachments(owner_id, state);

CREATE TABLE media_upload_attempts (
    id uuid PRIMARY KEY,
    attachment_id uuid NOT NULL REFERENCES attachments(id),
    staging_key text NOT NULL UNIQUE,
    grant_expires_at timestamptz NOT NULL,
    sealed_etag text,
    reserved_bytes bigint NOT NULL CHECK (reserved_bytes > 0),
    released_at timestamptz,
    deleted_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);
ALTER TABLE attachments ADD CONSTRAINT active_upload_attempt
    FOREIGN KEY (active_attempt_id) REFERENCES media_upload_attempts(id);

CREATE TABLE media_jobs (
    attachment_id uuid PRIMARY KEY REFERENCES attachments(id),
    attempt_id uuid NOT NULL REFERENCES media_upload_attempts(id),
    generation bigint NOT NULL DEFAULT 0,
    lease_token uuid,
    lease_until timestamptz,
    available_at timestamptz NOT NULL DEFAULT now(),
    failures integer NOT NULL DEFAULT 0,
    done boolean NOT NULL DEFAULT false
);

-- Every candidate is recorded before a network write. A lease never reuses a key.
CREATE TABLE media_objects (
    id uuid PRIMARY KEY,
    attachment_id uuid NOT NULL REFERENCES attachments(id),
    generation bigint NOT NULL,
    lease_token uuid NOT NULL,
    variant text NOT NULL CHECK (variant IN ('original', 'thumbnail', 'avatar')),
    object_key text NOT NULL UNIQUE,
    published boolean NOT NULL DEFAULT false,
    deletion_pending boolean NOT NULL DEFAULT false,
    deleted_at timestamptz,
    cleanup_after timestamptz NOT NULL,
    size_bytes bigint,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX published_media_variant ON media_objects(attachment_id, variant)
    WHERE published AND deleted_at IS NULL;

CREATE TABLE message_attachments (
    message_id uuid NOT NULL REFERENCES messages(id),
    attachment_id uuid NOT NULL REFERENCES attachments(id),
    position smallint NOT NULL CHECK (position BETWEEN 0 AND 9),
    PRIMARY KEY (message_id, position),
    UNIQUE (message_id, attachment_id)
);
CREATE INDEX message_attachment_references ON message_attachments(attachment_id);

CREATE TABLE user_avatars (
    user_id uuid PRIMARY KEY REFERENCES users(id),
    attachment_id uuid NOT NULL REFERENCES attachments(id),
    updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX avatar_attachment_references ON user_avatars(attachment_id);
