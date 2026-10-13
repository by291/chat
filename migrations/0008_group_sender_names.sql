-- Group history keeps the attribution captured when a message was saved.
ALTER TABLE messages ADD COLUMN sender_name text;
UPDATE messages message SET sender_name=sender.display_name
FROM users sender, conversations conversation
WHERE sender.id=message.sender_id AND conversation.id=message.conversation_id
      AND conversation.kind='group';
