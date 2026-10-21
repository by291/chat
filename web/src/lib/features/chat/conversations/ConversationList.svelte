<script lang="ts">
  import { onMount } from 'svelte';
  import { Search, MessageCircle, Pin, BellOff } from '@lucide/svelte';

  import * as Avatar from '#lib/components/ui/avatar/index.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import PaneHeader from '#lib/components/PaneHeader.svelte';
  import { avatarTone } from '#lib/utils.js';

  import ProfileAvatar from '#lib/features/settings/profile/ProfileAvatar.svelte';
  import WorkspaceNavigation from '#lib/features/workspace/WorkspaceNavigation.svelte';

  import NotificationSettings from '../notifications/NotificationSettings.svelte';
  import ConversationActions from './ConversationActions.svelte';
  import { isGroup, conversationName, type Conversation } from '../models';
  import { useChat } from '../context.svelte';
  import type { ChatSync } from '../sync.svelte';

  let {
    sync,
  }: {
    sync: ChatSync;
  } = $props();

  const chat = useChat();
  let list: HTMLElement;
  const filter = $derived(chat.listView.filter);
  const archived = $derived(chat.listView.archived);

  const archivedCount = $derived(
    sync.conversations.filter(
      (conversation) => conversation.preferences.archived,
    ).length,
  );
  let conversations = $derived(
    sync.conversations
      .filter(
        (conversation) =>
          conversation.preferences.archived === archived &&
          matchesFilter(conversation),
      )
      .sort(
        (left, right) =>
          Number(right.preferences.pinned) - Number(left.preferences.pinned),
      ),
  );

  function matchesFilter(conversation: Conversation): boolean {
    const username = isGroup(conversation) ? '' : conversation.peer.username;
    const searchableText = `${conversationName(conversation)} ${username}`;
    return searchableText.toLowerCase().includes(filter.toLowerCase());
  }

  const time = (value: string) =>
    new Date(value).toLocaleTimeString('zh-CN', {
      hour: '2-digit',
      minute: '2-digit',
    });

  onMount(() => {
    list.scrollTop = chat.listView.scrollTop;
  });
</script>

<aside class="conversation-sidebar" aria-label="会话列表">
  <WorkspaceNavigation />
  <PaneHeader title="聊天" level={1}>
    {#snippet actions()}
      <ConversationActions {sync} />
      <NotificationSettings {sync} />
    {/snippet}
  </PaneHeader>
  <div class="workspace-search">
    <Search size={16} aria-hidden="true" />
    <Input
      class="h-11 bg-background pl-10 text-base md:text-base dark:bg-background"
      aria-label="筛选会话"
      bind:value={chat.listView.filter}
      placeholder="搜索你的聊天"
    />
  </div>
  <div class="archive-tabs" aria-label="会话视图">
    <Button
      type="button"
      variant={archived ? 'ghost' : 'secondary'}
      aria-pressed={!archived}
      onclick={() => (chat.listView.archived = false)}
    >
      聊天
    </Button>
    <Button
      type="button"
      variant={archived ? 'secondary' : 'ghost'}
      aria-pressed={archived}
      onclick={() => (chat.listView.archived = true)}
    >
      归档 ({archivedCount})
    </Button>
  </div>
  <nav
    class="conversation-nav"
    aria-label="会话列表"
    bind:this={list}
    onscroll={() => (chat.listView.scrollTop = list.scrollTop)}
  >
    {#each conversations as conversation (conversation.id)}
      <a
        class="conversation-row"
        class:active={sync.selectedId === conversation.id}
        href={`/app/chats/${conversation.id}`}
        aria-current={sync.selectedId === conversation.id ? 'page' : undefined}
      >
        {#if isGroup(conversation)}
          <Avatar.Root data-tone={avatarTone(conversation.id)}>
            <Avatar.Fallback>
              {Array.from(conversationName(conversation))[0]}
            </Avatar.Fallback>
          </Avatar.Root>
        {:else}
          <ProfileAvatar
            userId={conversation.peer.id}
            name={conversation.peer.display_name}
          />
        {/if}
        <div class="conversation-copy">
          <div class="conversation-name">
            <strong>{conversationName(conversation)}</strong>
            {#if conversation.preferences.pinned}
              <Pin size={12} aria-label="已置顶" />
            {/if}
            {#if conversation.preferences.muted}
              <BellOff size={12} aria-label="已静音" />
            {/if}
            {#if conversation.last_message}
              <time>
                {time(conversation.last_message.created_at)}
              </time>
            {/if}
          </div>
          <span class="conversation-username">
            {#if isGroup(conversation)}
              群聊 · {conversation.member_count} 位成员
            {:else}
              @{conversation.peer.username}
            {/if}
          </span>
          <div class="conversation-preview">
            <p>
              {sync.typingByConversation[conversation.id]
                ? '正在输入…'
                : conversation.last_message?.withdrawn
                  ? '此消息已撤回'
                  : (conversation.last_message?.body ?? '开始你们的第一段对话')}
            </p>
            {#if conversation.unread_count > 0}
              <Badge>
                {conversation.unread_count > 99
                  ? '99+'
                  : conversation.unread_count}
              </Badge>
            {/if}
          </div>
        </div>
      </a>
    {/each}
    {#if !conversations.length}
      <div class="sidebar-empty">
        <MessageCircle size={28} />
        <p>
          {filter
            ? '没有匹配的聊天'
            : archived
              ? '没有归档的聊天'
              : '还没有聊天'}
        </p>
        <span>
          {filter ? '试试另一位伙伴的名字' : '从通讯录选择好友开始聊天'}
        </span>
        {#if !filter && !archived}<Button
            variant="secondary"
            href="/app/contacts"
          >
            去添加好友
          </Button>{/if}
      </div>
    {/if}
  </nav>
</aside>
