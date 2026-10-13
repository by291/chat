CREATE TABLE conversations(
    id uuid PRIMARY KEY,
    kind text NOT NULL DEFAULT 'direct' CHECK(kind IN('direct','group')),
    min_user_id uuid REFERENCES users(id),
    max_user_id uuid REFERENCES users(id),
    latest_event_seq bigint NOT NULL DEFAULT 0 CHECK(latest_event_seq>=0),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE(min_user_id,max_user_id),
    CHECK(kind!='direct' OR min_user_id<max_user_id)
);
CREATE TABLE conversation_members(
    conversation_id uuid NOT NULL REFERENCES conversations(id),
    user_id uuid NOT NULL REFERENCES users(id),
    role text NOT NULL DEFAULT 'member',
    joined_seq bigint NOT NULL DEFAULT 1,
    last_read_message_seq bigint NOT NULL DEFAULT 0,
    left_at timestamptz,
    PRIMARY KEY(conversation_id,user_id)
);
CREATE TABLE conversation_events(
    event_id uuid PRIMARY KEY,
    conversation_id uuid NOT NULL REFERENCES conversations(id),
    event_seq bigint NOT NULL CHECK(event_seq>0),
    kind text NOT NULL,
    payload jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE(conversation_id,event_seq)
);
CREATE INDEX members_user ON conversation_members(user_id,conversation_id);
