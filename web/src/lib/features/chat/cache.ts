import { QueryClient } from '@tanstack/svelte-query';

const QUERY_STALE_TIME_MS = 30_000;

export const keys = {
  members: (id: string) => ['members', id] as const,
  conversations: ['conversations'] as const,
  messages: (id: string) => ['messages', id] as const,
};

/** Canonical records live until their membership/session owners explicitly remove them. */
export function createQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: {
        staleTime: QUERY_STALE_TIME_MS,
        retry: 1,
        // Manual replay/media writes have no QueryObservers to pin their records.
        // Timer eviction would leave applied cursors or upload drafts without data;
        // membership invalidation and authenticated teardown own their removal.
        gcTime: Infinity,
      },
    },
  });
}
