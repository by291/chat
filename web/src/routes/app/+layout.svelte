<script lang="ts">
  import { onMount, untrack, type Snippet } from 'svelte';
  import { goto } from '$app/navigation';
  import { page } from '$app/state';
  import { QueryClientProvider } from '@tanstack/svelte-query';

  import { api } from '#lib/api-client';

  import { provideContacts } from '#lib/features/contacts/context.svelte.ts';
  import { ContactsState } from '#lib/features/contacts/contacts.svelte.ts';

  import { auth } from '#lib/features/auth/session.svelte.ts';
  import { createQueryClient } from '#lib/features/chat/cache.ts';
  import { createChatApi } from '#lib/features/chat/api.ts';
  import { createChatSync } from '#lib/features/chat/sync.svelte.ts';
  import { createRealtimeTransport } from '#lib/features/chat/realtime.ts';
  import { provideChat } from '#lib/features/chat/context.svelte.ts';
  import ChatShell from '#lib/features/chat/ChatShell.svelte';
  import WorkspaceNavigation from '#lib/features/chat/WorkspaceNavigation.svelte';
  import NotificationCenter from '#lib/features/chat/NotificationCenter.svelte';
  import UpdateNotice from '#lib/features/pwa/UpdateNotice.svelte';
  import { provideBackgroundPush } from '#lib/features/notifications/context.svelte.ts';
  import { PushSubscriptions } from '#lib/features/notifications/subscriptions.svelte.ts';
  import { createNotificationApi } from '#lib/features/notifications/api.ts';

  const SESSION_REFRESH_INTERVAL_MS = 60_000;

  let {
    children,
  }: {
    children: Snippet;
  } = $props();

  const queryClient = createQueryClient();
  const chat = provideChat();
  const contacts = provideContacts();
  const backgroundPush = provideBackgroundPush();

  // Profile revalidation replaces the user object. Only an identity change may
  // recreate the coordinator, otherwise the same user's unsent drafts are lost.
  const authenticatedId = $derived(auth.user?.id);
  const isChatRoute = $derived(
    page.url.pathname === '/app' || page.url.pathname.startsWith('/app/chats/'),
  );
  const hasDraft = $derived(
    Object.values(chat.sync?.drafts ?? {}).some((text) => !!text) ||
      !!chat.sync?.actions.editingId,
  );
  const hasUpload = $derived(!!chat.sync?.uploads.entries.length);
  const hasSubmission = $derived(
    chat.sync?.pending.some((message) => message.status === 'sending') ||
      chat.sync?.actions.busy ||
      !!chat.sync?.actions.reacting.length ||
      false,
  );

  onMount(() => {
    void auth.restore();

    const restore = async () => {
      if (document.visibilityState !== 'visible') {
        return;
      }
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
    if (!auth.pending && !auth.user) {
      const pendingJoin =
        page.url.pathname === '/app/groups/join' ||
        /^\/app\/chats\/[0-9a-f-]{36}$/.test(page.url.pathname)
          ? `?next=${encodeURIComponent(page.url.pathname + page.url.search)}`
          : '';
      void goto(`/login${pendingJoin}`);
    }
  });

  $effect(() => {
    const userId = authenticatedId;
    if (!userId) return;

    const owner = new PushSubscriptions(createNotificationApi());
    backgroundPush.owner = owner;
    const removeReset = auth.onReset(() => void owner.stop());
    // Optional capability startup reads UI flags; identity alone owns this lifetime.
    untrack(() => void owner.start());

    return () => {
      removeReset();
      // Leaving /app can preserve authentication (for example /login redirects).
      // Only identity replacement/reset revokes the shared browser capability.
      if (auth.user?.id !== userId) {
        void owner.stop();
      } else {
        owner.dispose();
      }
      backgroundPush.owner = null;
    };
  });

  $effect(() => {
    const owner = backgroundPush.owner;
    const conversation = page.url.pathname.startsWith('/app/chats/')
      ? (chat.sync?.selectedId ?? null)
      : null;
    if (!owner) return;

    untrack(() =>
      owner.setActivity({
        visible: document.visibilityState === 'visible',
        focused: document.hasFocus(),
        conversation_id: conversation,
      }),
    );
  });

  $effect(() => {
    const userId = authenticatedId;
    if (!userId) {
      return;
    }

    const contactOwner = new ContactsState({ queryClient, userId });
    contacts.owner = contactOwner;
    untrack(() => void contactOwner.start());
    const sync = createChatSync({
      api: createChatApi(api),
      transport: createRealtimeTransport(userId, () => auth.reset()),
      queryClient,
      userId,
      onContactsChanged: () => void contactOwner.refresh(),
      onReconnect: () => void contactOwner.refresh(),
    });
    chat.sync = sync;
    // Starting reads reactive coordination fields; these must not become effect
    // dependencies that repeatedly tear down the authenticated session lifetime.
    untrack(() => void sync.start());

    return () => {
      contactOwner.stop();
      contacts.owner = null;
      contacts.view = { category: 'friends', selectedId: null, scrollTop: 0 };
      sync.stop();
      chat.sync = null;
      chat.listView = { filter: '', archived: false, scrollTop: 0 };
      chat.readingPositions.clear();
    };
  });
</script>

{#if auth.user}
  <QueryClientProvider client={queryClient}>
    <div class="app-frame">
      <WorkspaceNavigation />
      <div class="app-content">
        {#if isChatRoute}
          <ChatShell id={page.params.id ?? null} />
        {:else}
          {@render children()}
        {/if}
      </div>
      {#if chat.sync}
        <NotificationCenter sync={chat.sync} />
      {/if}
      <UpdateNotice {hasDraft} {hasUpload} {hasSubmission} />
    </div>
  </QueryClientProvider>
{:else}
  <main class="loading-page">
    <span class="brand-mark">C</span>
    <p>正在恢复登录状态…</p>
    {#if auth.error}
      <p role="alert">{auth.error}</p>
    {/if}
  </main>
{/if}
