<script lang="ts">
  import * as Avatar from '#lib/components/ui/avatar/index.js';
  import { Button } from '#lib/components/ui/button/index.js';

  import { onMount, untrack, type Snippet } from 'svelte';
  import { goto } from '$app/navigation';
  import { page } from '$app/state';
  import { MessageCircle, Settings2, LogOut } from '@lucide/svelte';
  import { auth } from '#lib/features/auth/session.svelte.ts';
  import { QueryClientProvider } from '@tanstack/svelte-query';
  import { api } from '#lib/api-client';
  import { createQueryClient } from '#lib/features/chat/cache.ts';
  import { createChatApi } from '#lib/features/chat/api.ts';
  import { createChatSync } from '#lib/features/chat/sync.svelte.ts';
  import { createRealtimeTransport } from '#lib/features/chat/realtime.ts';
  import { provideChat } from '#lib/features/chat/context.svelte.ts';
  import NotificationCenter from '#lib/features/chat/NotificationCenter.svelte';
  import UpdateNotice from '#lib/features/pwa/UpdateNotice.svelte';
  import { provideBackgroundPush } from '#lib/features/notifications/context.svelte.ts';
  import { PushSubscriptions } from '#lib/features/notifications/subscriptions.svelte.ts';
  import { createNotificationApi } from '#lib/features/notifications/api.ts';

  const queryClient = createQueryClient();
  const chat = provideChat();
  const backgroundPush = provideBackgroundPush();

  let {
    children,
  }: {
    children: Snippet;
  } = $props();

  let error = $state('');

  // Profile revalidation replaces the user object. Only an identity change may
  // recreate the coordinator, otherwise the same user's unsent drafts are lost.
  const authenticatedId = $derived(auth.user?.id);

  onMount(() => {
    void auth.restore();
    const restore = async () => {
      if (document.visibilityState !== 'visible') {
        return;
      }
      await auth.restore();
      if (document.visibilityState === 'visible') {
        await chat.sync?.refresh();
      }
    };
    const interval = setInterval(restore, 60000);
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
      if (auth.user?.id !== userId) void owner.stop();
      else owner.dispose();
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
    const sync = createChatSync({
      api: createChatApi(api),
      transport: createRealtimeTransport(userId, () => auth.reset()),
      queryClient,
      userId,
    });
    chat.sync = sync;
    // Starting reads reactive coordination fields; these must not become effect
    // dependencies that repeatedly tear down the authenticated session lifetime.
    untrack(() => void sync.start());
    return () => {
      sync.stop();
      chat.sync = null;
    };
  });

  async function logout() {
    try {
      await backgroundPush.owner?.disable();
      await auth.logout();
      await goto('/login');
    } catch (reason) {
      error = reason instanceof Error ? reason.message : '退出失败';
    }
  }
</script>

{#if auth.user}
  <QueryClientProvider client={queryClient}>
    <div class="app-frame">
      <nav class="app-rail" aria-label="主导航">
        <a class="brand-mark" href="/app" aria-label="聊天">
          <MessageCircle size={24} />
        </a>
        <div class="rail-bottom">
          <Button
            variant="ghost"
            size="icon"
            href="/app/settings"
            aria-label="设置"
          >
            <Settings2 size={22} />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            aria-label="退出登录"
            onclick={logout}
          >
            <LogOut size={21} />
          </Button>
          <Avatar.Root size="sm" title={auth.user.display_name}>
            <Avatar.Fallback>
              {auth.user.display_name.slice(0, 1)}
            </Avatar.Fallback>
          </Avatar.Root>
        </div>
      </nav>
      <div class="app-content">
        {@render children()}
        {#if error}
          <p class="toast error" role="alert">
            {error}
          </p>
        {/if}
      </div>
      {#if chat.sync}
        <NotificationCenter sync={chat.sync} />
      {/if}
      <UpdateNotice
        hasDraft={Object.values(chat.sync?.drafts ?? {}).some(
          (text) => !!text,
        ) || !!chat.sync?.actions.editingId}
        hasUpload={!!chat.sync?.uploads.entries.length}
        hasSubmission={chat.sync?.pending.some(
          (message) => message.status === 'sending',
        ) ||
          chat.sync?.actions.busy ||
          !!chat.sync?.actions.reacting.length ||
          false}
      />
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
