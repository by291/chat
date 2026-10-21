import type { QueryClient } from '@tanstack/svelte-query';

import { api } from '#lib/api-client';
import { auth } from '#lib/features/auth/session.svelte.ts';
import { createChatApi } from '#lib/features/chat/api.ts';
import type { provideChat } from '#lib/features/chat/context.svelte.ts';
import { createRealtimeTransport } from '#lib/features/chat/synchronization/realtime.ts';
import { createChatSync } from '#lib/features/chat/sync.svelte.ts';
import { ContactsState } from '#lib/features/contacts/contacts.svelte.ts';
import type { provideContacts } from '#lib/features/contacts/context.svelte.ts';
import { createNotificationApi } from '#lib/features/notifications/api.ts';
import type { provideBackgroundPush } from '#lib/features/notifications/context.svelte.ts';
import { PushSubscriptions } from '#lib/features/notifications/subscriptions.svelte.ts';

type WorkspaceSessionOptions = {
  userId: string;
  queryClient: QueryClient;
  chat: ReturnType<typeof provideChat>;
  contacts: ReturnType<typeof provideContacts>;
  backgroundPush: ReturnType<typeof provideBackgroundPush>;
};

/**
 * Start one identity's feature owners and return their ordered teardown.
 * The workspace owns the complete cache; feature disposal removes only its keys.
 */
export function startWorkspaceSession({
  userId,
  queryClient,
  chat,
  contacts,
  backgroundPush,
}: WorkspaceSessionOptions) {
  const pushOwner = new PushSubscriptions(createNotificationApi());
  const contactOwner = new ContactsState({ queryClient, userId });
  const sync = createChatSync({
    api: createChatApi(api),
    transport: createRealtimeTransport(userId, () => auth.reset()),
    queryClient,
    userId,
    onContactsChanged: () => void contactOwner.refresh(),
    onReconnect: () => void contactOwner.refresh(),
  });
  backgroundPush.owner = pushOwner;
  contacts.owner = contactOwner;
  chat.sync = sync;

  const removeReset = auth.onReset(() => void pushOwner.stop());
  void pushOwner.start();
  void contactOwner.start();
  void sync.start();

  return () => {
    removeReset();
    // Leaving /app may preserve the cookie. Revocation belongs to identity
    // reset/replacement; ordinary route disposal retains browser capability.
    if (auth.user?.id !== userId) {
      void pushOwner.stop();
    } else {
      pushOwner.dispose();
    }
    contactOwner.stop();
    sync.stop();

    backgroundPush.owner = null;
    contacts.owner = null;
    contacts.view = { category: 'friends', selectedId: null, scrollTop: 0 };
    chat.sync = null;
    chat.listView = { filter: '', archived: false, scrollTop: 0 };
    chat.readingPositions.clear();
    queryClient.clear();
  };
}
