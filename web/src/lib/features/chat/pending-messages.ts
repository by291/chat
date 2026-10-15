import type { components } from '#lib/api-client';

import type { Message, PendingMessage } from './models';

/** Freeze the original send payload; later draft/reply/upload changes must not change a retry. */
export function createPendingMessage(input: {
  conversationId: string;
  clientMessageId: string;
  body: string;
  membershipId?: string;
  attachmentIds: string[];
  replyToMessageId?: string;
}): PendingMessage {
  return {
    conversation_id: input.conversationId,
    client_message_id: input.clientMessageId,
    body: input.body,
    status: 'sending',
    created_at: new Date().toISOString(),
    membership_id: input.membershipId,
    attachment_ids: [...input.attachmentIds],
    ...(input.replyToMessageId
      ? { reply_to_message_id: input.replyToMessageId }
      : {}),
  };
}

/** Construct HTTP input from the frozen pending record, including retries after a lost response. */
export function pendingMessageInput(
  pending: PendingMessage,
): components['schemas']['SendMessageInput'] {
  return {
    client_message_id: pending.client_message_id,
    body: pending.body,
    membership_id: pending.membership_id,
    ...(pending.reply_to_message_id
      ? { reply_to_message_id: pending.reply_to_message_id }
      : {}),
    ...(pending.attachment_ids.length
      ? { attachment_ids: pending.attachment_ids }
      : {}),
  };
}

/** Only our sender/client-ID pair acknowledges a send; another member may choose the same UUID. */
export function unacknowledgedMessages(
  pending: PendingMessage[],
  incoming: Message[],
  currentUserId: string,
): PendingMessage[] {
  return pending.filter(
    (submission) =>
      !incoming.some(
        (message) =>
          message.conversation_id === submission.conversation_id &&
          message.sender_id === currentUserId &&
          message.client_message_id === submission.client_message_id,
      ),
  );
}
