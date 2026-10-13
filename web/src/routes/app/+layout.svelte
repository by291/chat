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

  const queryClient = createQueryClient();
  const chat = provideChat();

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
        page.url.pathname === '/app/groups/join'
          ? `?next=${encodeURIComponent(page.url.pathname + page.url.search)}`
          : '';
      void goto(`/login${pendingJoin}`);
    }
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
