CREATE TABLE realtime_connections(
    client_id text PRIMARY KEY,
    session_id uuid NOT NULL REFERENCES sessions(id),
    user_id uuid NOT NULL REFERENCES users(id),
    expires_at timestamptz NOT NULL
);
CREATE INDEX realtime_session ON realtime_connections(session_id);
