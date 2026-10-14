-- Retry identity belongs to the original submission, not mutable display content.
ALTER TABLE messages ADD COLUMN original_body_hash text;
ALTER TABLE messages ADD COLUMN original_attachment_ids uuid[] NOT NULL DEFAULT '{}';
ALTER TABLE messages ADD COLUMN reply_to_message_id uuid REFERENCES messages(id);
ALTER TABLE messages ADD COLUMN revision_seq bigint;
ALTER TABLE messages ADD COLUMN edited_at timestamptz;
ALTER TABLE messages ADD COLUMN withdrawn boolean NOT NULL DEFAULT false;

UPDATE messages
SET original_body_hash = encode(sha256(convert_to(body, 'UTF8')), 'hex'), revision_seq = created_seq;
UPDATE messages message
SET original_attachment_ids = reference.attachment_ids
FROM (
    SELECT message_id, array_agg(attachment_id ORDER BY position) AS attachment_ids
    FROM message_attachments
    GROUP BY message_id
) reference
WHERE message.id = reference.message_id;

ALTER TABLE messages ALTER COLUMN original_body_hash SET NOT NULL;
ALTER TABLE messages ALTER COLUMN revision_seq SET NOT NULL;
ALTER TABLE messages ADD CONSTRAINT message_revision_valid CHECK (revision_seq >= created_seq);
ALTER TABLE messages ADD CONSTRAINT withdrawn_message_empty CHECK (NOT withdrawn OR body = '');

-- Preferences outlive membership episodes and never enter the conversation stream.
CREATE TABLE conversation_preferences (
    conversation_id uuid NOT NULL REFERENCES conversations(id),
    user_id uuid NOT NULL REFERENCES users(id),
    pinned boolean NOT NULL DEFAULT false,
    muted boolean NOT NULL DEFAULT false,
    archived boolean NOT NULL DEFAULT false,
    revision bigint NOT NULL DEFAULT 0,
    PRIMARY KEY (conversation_id, user_id)
);
