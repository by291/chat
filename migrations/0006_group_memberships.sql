-- Retain ended membership episodes so rejoining cannot restore an earlier history boundary.
ALTER TABLE conversations ADD COLUMN title text;
ALTER TABLE conversations ADD CONSTRAINT group_title_valid
    CHECK (kind != 'group' OR (title IS NOT NULL AND char_length(title) BETWEEN 1 AND 100));

ALTER TABLE conversation_members ADD COLUMN membership_id uuid NOT NULL DEFAULT gen_random_uuid();
ALTER TABLE conversation_members ADD COLUMN left_seq bigint;
ALTER TABLE conversation_members DROP CONSTRAINT conversation_members_pkey;
ALTER TABLE conversation_members ADD PRIMARY KEY (membership_id);
ALTER TABLE conversation_members ADD CONSTRAINT member_role_valid CHECK (role IN ('owner','admin','member'));
ALTER TABLE conversation_members ADD CONSTRAINT member_join_positive CHECK (joined_seq > 0);
ALTER TABLE conversation_members ADD CONSTRAINT member_leave_boundary
    CHECK (left_seq IS NULL OR left_seq >= joined_seq);

CREATE UNIQUE INDEX active_conversation_member
    ON conversation_members(conversation_id,user_id) WHERE left_at IS NULL;
CREATE UNIQUE INDEX active_group_owner
    ON conversation_members(conversation_id) WHERE left_at IS NULL AND role = 'owner';
