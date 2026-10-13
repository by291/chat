CREATE TABLE group_invitations (
    id uuid PRIMARY KEY,
    conversation_id uuid NOT NULL REFERENCES conversations(id),
    issuer_id uuid NOT NULL REFERENCES users(id),
    token_hash text NOT NULL UNIQUE,
    expires_at timestamptz NOT NULL,
    revoked_at timestamptz,
    consumed_at timestamptz,
    consumed_by uuid REFERENCES users(id),
    CHECK ((consumed_at IS NULL) = (consumed_by IS NULL))
);
CREATE INDEX group_invitation_conversation ON group_invitations(conversation_id);
