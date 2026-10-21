import { onMount, untrack } from 'svelte';

import { auth } from '#lib/features/auth/session.svelte.ts';
import { createQueryClient } from '#lib/features/chat/cache.ts';
import { provideChat } from '#lib/features/chat/context.svelte.ts';
import { provideContacts } from '#lib/features/contacts/context.svelte.ts';
import { provideBackgroundPush } from '#lib/features/notifications/context.svelte.ts';

import { startWorkspaceSession } from './session';

const SESSION_REFRESH_INTERVAL_MS = 60_000;

/**
 * Compose authenticated feature owners during component initialization.
 * Profile replacements preserve this lifetime; identity changes stop every owner
 * before clearing the shared cache, including entries outside the chat feature.
 */
export function createWorkspaceLifetime() {
  const queryClient = createQueryClient();
  const chat = provideChat();
  const contacts = provideContacts();
  const backgroundPush = provideBackgroundPush();
  const authenticatedId = $derived(auth.user?.id);

  onMount(() => {
    void auth.restore();

    const restore = async () => {
      if (document.visibilityState !== 'visible') return;

      await auth.restore();
      if (document.visibilityState === 'visible') {
        await chat.sync?.refresh();
        await contacts.owner?.refresh();
      }
    };
    const interval = setInterval(restore, SESSION_REFRESH_INTERVAL_MS);
    window.addEventListener('focus', restore);
    document.addEventListener('visibilitychange', restore);

    return () => {
      clearInterval(interval);
      window.removeEventListener('focus', restore);
      document.removeEventListener('visibilitychange', restore);
    };
  });

  $effect(() => {
    const userId = authenticatedId;
    if (!userId) return;

    // Construction and startup publish cache/reactive changes. Only identity
    // owns this effect; subscribing to those changes would discard live drafts.
    return untrack(() =>
      startWorkspaceSession({
        userId,
        queryClient,
        chat,
        contacts,
        backgroundPush,
      }),
    );
  });

  return { queryClient, chat, contacts, backgroundPush };
}
