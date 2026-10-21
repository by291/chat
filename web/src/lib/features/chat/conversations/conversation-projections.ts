import { mergeMessageRecords } from '../messages/message-records';
import type { Conversation, Message } from '../models';
import { isGroup, membershipId, sequence } from '../models';

/** Preserve monotonic decimal-string cursors without passing through Number. */
export function latestSequence(existing: string, incoming: string): string {
  return sequence(existing) > sequence(incoming) ? existing : incoming;
}

/** Summary snapshots share message revision fences, but never establish replay continuity. */
export function reconcileSummaryPreview(
  incoming: Conversation,
  previous: Conversation | undefined,
  messages: Message[],
): Conversation {
  const previousMessage =
    membershipId(previous) === membershipId(incoming)
      ? previous?.last_message
      : undefined;
  const candidates = [
    previousMessage,
    messages.at(-1),
    incoming.last_message,
  ].filter((message): message is Message => !!message);

  return candidates.length
    ? { ...incoming, last_message: mergeMessageRecords([], candidates).at(-1)! }
    : incoming;
}

/** A group summary stores only the actor's receipt; peer receipts belong to its roster. */
export function applySummaryReceipt(
  conversation: Conversation,
  receiptUserId: string,
  receiptSequence: string,
  currentUserId: string,
): Conversation {
  const ownReceipt = receiptUserId === currentUserId;
  if (isGroup(conversation)) {
    return ownReceipt
      ? {
          ...conversation,
          my_read_seq: latestSequence(
            conversation.my_read_seq,
            receiptSequence,
          ),
        }
      : conversation;
  }

  return ownReceipt
    ? {
        ...conversation,
        my_read_seq: latestSequence(conversation.my_read_seq, receiptSequence),
      }
    : {
        ...conversation,
        peer_read_seq: latestSequence(
          conversation.peer_read_seq,
          receiptSequence,
        ),
      };
}

/** Refresh a known last-message projection without replacing it with unrelated older history. */
export function updateSummaryPreview(
  conversation: Conversation,
  messages: Message[],
): Conversation {
  const previous = conversation.last_message;
  if (!previous) return conversation;

  const current = messages.find((message) => message.id === previous.id);
  return current
    ? {
        ...conversation,
        last_message: mergeMessageRecords([previous], [current])[0],
      }
    : conversation;
}
