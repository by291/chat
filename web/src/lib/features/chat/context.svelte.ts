import { getContext, setContext } from 'svelte';

import type { ChatSync } from './sync.svelte';

const key = Symbol('chat');

/** The authenticated route provides this context; logout clears its coordinator. */
export class ChatContext {
  sync = $state<ChatSync | null>(null);
}

export function provideChat() {
  const context = new ChatContext();
  setContext(key, context);
  return context;
}

export function useChat() {
  return getContext<ChatContext>(key);
}
