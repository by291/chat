CREATE TABLE messages(
    id uuid PRIMARY KEY,
    conversation_id uuid NOT NULL REFERENCES conversations(id),
    sender_id uuid NOT NULL REFERENCES users(id),
    client_message_id uuid NOT NULL,
    created_seq bigint NOT NULL CHECK(created_seq>0),
    body text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE(sender_id,conversation_id,client_message_id),
    UNIQUE(conversation_id,created_seq)
);
CREATE INDEX messages_history ON messages(conversation_id,created_seq DESC);
