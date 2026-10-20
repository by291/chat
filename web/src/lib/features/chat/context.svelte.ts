import { getContext, setContext } from 'svelte';

import type { ChatSync } from './sync.svelte';

const key = Symbol('chat');

export interface ReadingPosition {
  membershipId: string | undefined;
  anchorId: string | undefined;
  offset: number;
  scrollTop: number;
  following: boolean;
}

/** The authenticated route provides this context; logout clears its coordinator. */
export class ChatContext {
  sync = $state<ChatSync | null>(null);
  listView = $state({ filter: '', archived: false, scrollTop: 0 });
  // DOM positions are session UI state, never server records or read receipts.
  readonly readingPositions = new Map<string, ReadingPosition>();
}

export function provideChat() {
  const context = new ChatContext();
  setContext(key, context);
  return context;
}

export function useChat() {
  return getContext<ChatContext>(key);
}
