<script lang="ts">
  import * as Avatar from '#lib/components/ui/avatar/index.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { Input } from '#lib/components/ui/input/index.js';

  import ProfileAvatar from '../settings/ProfileAvatar.svelte';
  import { Search, MessageCircle } from '@lucide/svelte';
  import StartConversationDialog from './StartConversationDialog.svelte';
  import GroupDialog from './GroupDialog.svelte';
  import { isGroup, conversationName } from './models';
  import type { ChatSync } from './sync.svelte';

  let {
    sync,
  }: {
    sync: ChatSync;
  } = $props();

  let filter = $state('');

  let conversations = $derived(
    sync.conversations.filter((c) =>
      `${conversationName(c)} ${isGroup(c) ? '' : c.peer.username}`
        .toLowerCase()
        .includes(filter.toLowerCase()),
    ),
  );

  const time = (value: string) =>
    new Date(value).toLocaleTimeString('zh-CN', {
      hour: '2-digit',
      minute: '2-digit',
    });
</script>

<aside class="conversation-sidebar">
  <header class="sidebar-heading">
    <div>
      <span class="eyebrow">YOUR EVERYDAY SPACE</span>
      <h1>
        聊天
        <span class="heading-dot">.</span>
      </h1>
    </div>
    <div class="group-actions">
      <StartConversationDialog {sync} />
      <GroupDialog {sync} />
    </div>
  </header>
  <div class="search-field">
    <Search size={16} />
    <Input
      aria-label="筛选会话"
      bind:value={filter}
      placeholder="搜索你的聊天"
    />
  </div>
  <div class="connection-status" aria-live="polite">
    <span
      class:connected={sync.connection === 'online'}
      class="connection-dot"
    ></span>
    {sync.connection === 'online'
      ? '已连接'
      : sync.connection === 'connecting'
        ? '正在连接'
        : '实时连接中断，正在重连'}
  </div>
  <nav class="conversation-nav" aria-label="会话列表">
    {#each conversations as conversation (conversation.id)}
      <a
        class="conversation-row"
        class:active={sync.selectedId === conversation.id}
        href={`/app/chats/${conversation.id}`}
        aria-current={sync.selectedId === conversation.id ? 'page' : undefined}
      >
        {#if isGroup(conversation)}
          <Avatar.Root>
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
        <p>{filter ? '没有匹配的聊天' : '还没有聊天'}</p>
        <span>
          {filter ? '试试另一位伙伴的名字' : '点击右上角，找到你的伙伴'}
        </span>
      </div>
    {/if}
  </nav>
  <footer class="sidebar-footer">轻松联系，认真交谈。</footer>
</aside>
