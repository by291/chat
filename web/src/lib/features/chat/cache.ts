import { QueryClient } from '@tanstack/svelte-query';

export const keys = {
  conversations: ['conversations'] as const,
  messages: (id: string) => ['messages', id] as const,
};

export function createQueryClient() {
  return new QueryClient({
    defaultOptions: { queries: { staleTime: 30000, retry: 1, gcTime: 300000 } },
  });
}
