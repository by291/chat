<script lang="ts">
  import { page } from '$app/state';
  import { MessageCircle, Settings2, Users } from '@lucide/svelte';

  import { Button } from '#lib/components/ui/button/index.js';
  import AccountMenu from '#lib/features/settings/AccountMenu.svelte';
  import { useContacts } from '#lib/features/contacts/context.svelte.ts';
  import { useChat } from './context.svelte';
  import { auth } from '#lib/features/auth/session.svelte.ts';

  const chat = useChat();
  const contacts = useContacts();
  const chatHref = $derived(
    chat.sync?.selectedId ? `/app/chats/${chat.sync.selectedId}` : '/app',
  );
  const contactHref = $derived(
    contacts.view.selectedId
      ? `/app/contacts?category=${contacts.view.category}&person=${contacts.view.selectedId}`
      : `/app/contacts?category=${contacts.view.category}`,
  );
  const active = $derived(
    page.url.pathname.startsWith('/app/settings')
      ? 'settings'
      : page.url.pathname.startsWith('/app/contacts')
        ? 'contacts'
        : 'chat',
  );
</script>

<header class="workspace-identity">
  <AccountMenu />
  <div class="workspace-identity-copy">
    <strong>{auth.user?.display_name}</strong>
    <span>我的私人空间</span>
  </div>
</header>
<nav aria-label="主导航" class="workspace-navigation">
  <Button
    variant="ghost"
    class="workspace-link"
    href={chatHref}
    aria-label="聊天"
    title="聊天"
    aria-current={active === 'chat' ? 'page' : undefined}
  >
    <MessageCircle class="size-5" />
    <span class="workspace-label">消息</span>
  </Button>
  <Button
    variant="ghost"
    class="workspace-link"
    href={contactHref}
    aria-label={contacts.owner?.pendingCount
      ? `通讯录，${contacts.owner.pendingCount} 条好友申请`
      : '通讯录'}
    title="通讯录"
    aria-current={active === 'contacts' ? 'page' : undefined}
  >
    <Users class="size-5" />
    <span class="workspace-label">通讯录</span>
    {#if contacts.owner?.pendingCount}<span class="workspace-badge">
        {contacts.owner.pendingCount}
      </span>{/if}
  </Button>
  <Button
    variant="ghost"
    class="workspace-link"
    href="/app/settings"
    aria-label="设置"
    title="设置"
    aria-current={active === 'settings' ? 'page' : undefined}
  >
    <Settings2 class="size-5" />
    <span class="workspace-label">设置</span>
  </Button>
</nav>
