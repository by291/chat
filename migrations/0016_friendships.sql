CREATE TABLE friendships (
    user_low uuid NOT NULL REFERENCES users(id),
    user_high uuid NOT NULL REFERENCES users(id),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_low, user_high),
    CHECK (user_low < user_high)
);
CREATE INDEX friendships_high ON friendships(user_high, user_low);

CREATE TABLE friend_requests (
    id uuid PRIMARY KEY,
    sender_id uuid NOT NULL REFERENCES users(id),
    recipient_id uuid NOT NULL REFERENCES users(id),
    user_low uuid GENERATED ALWAYS AS (LEAST(sender_id, recipient_id)) STORED,
    user_high uuid GENERATED ALWAYS AS (GREATEST(sender_id, recipient_id)) STORED,
    status text NOT NULL CHECK (status IN ('pending', 'accepted', 'declined', 'cancelled')),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CHECK (sender_id <> recipient_id)
);
CREATE UNIQUE INDEX friend_requests_pending_pair
    ON friend_requests(user_low, user_high) WHERE status = 'pending';
CREATE INDEX friend_requests_recipient ON friend_requests(recipient_id, id);
CREATE INDEX friend_requests_sender ON friend_requests(sender_id, id);
