<script lang="ts">
  import { untrack, type Snippet } from 'svelte';
  import { page } from '$app/state';
  import { QueryClientProvider } from '@tanstack/svelte-query';

  import { auth } from '#lib/features/auth/session.svelte.ts';
  import ChatShell from '#lib/features/chat/ChatShell.svelte';
  import NotificationCenter from '#lib/features/chat/notifications/NotificationCenter.svelte';
  import UpdateNotice from '#lib/features/pwa/UpdateNotice.svelte';

  import WorkspaceNavigation from './WorkspaceNavigation.svelte';
  import { createWorkspaceLifetime } from './lifetime.svelte';

  let { children }: { children: Snippet } = $props();

  const { queryClient, chat, backgroundPush } = createWorkspaceLifetime();

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
</script>

{#if auth.user}
  <QueryClientProvider client={queryClient}>
    <div class="app-frame">
      {#if page.url.pathname.startsWith('/app/groups/')}
        <aside class="standalone-sidebar"><WorkspaceNavigation /></aside>
      {/if}
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
