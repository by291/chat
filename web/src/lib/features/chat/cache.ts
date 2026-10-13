import { QueryClient } from '@tanstack/svelte-query';

export const keys = {
  members: (id: string) => ['members', id] as const,
  conversations: ['conversations'] as const,
  messages: (id: string) => ['messages', id] as const,
};

export function createQueryClient() {
  return new QueryClient({
    defaultOptions: { queries: { staleTime: 30000, retry: 1, gcTime: 300000 } },
  });
}
