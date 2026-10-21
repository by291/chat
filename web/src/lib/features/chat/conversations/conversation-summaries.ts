import type { ChatApi, Conversation } from '../models';

/** Collect one paginated attempt; the caller owns coalescing and revision/episode reconciliation. */
export async function collectConversationSummaries(options: {
  api: ChatApi;
  current: () => boolean;
  revoked: (conversation: Conversation) => boolean;
}): Promise<Map<string, Conversation> | undefined> {
  const summaries = new Map<string, Conversation>();
  const seenCursors = new Set<string>();
  let cursor: string | undefined;

  do {
    const page = await options.api.list(cursor);
    if (!options.current()) return;

    for (const conversation of page.items) {
      if (!options.revoked(conversation)) {
        summaries.set(conversation.id, conversation);
      }
    }

    cursor = page.next_cursor ?? undefined;
    if (cursor) {
      if (seenCursors.has(cursor)) {
        throw new Error('会话分页未能继续，请重试');
      }

      seenCursors.add(cursor);
    }
  } while (cursor);

  return summaries;
}
