<script lang="ts">
  import { Search, MessageCircle } from '@lucide/svelte';
  import StartConversationDialog from './StartConversationDialog.svelte';
  import type { ChatSync } from './sync.svelte';

  let {
    sync,
  }: {
    sync: ChatSync;
  } = $props();

  let filter = $state('');

  let conversations = $derived(
    sync.conversations.filter((c) =>
      `${c.peer.username} ${c.peer.display_name}`
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
    <StartConversationDialog {sync} />
  </header>
  <div class="search-field">
    <Search size={16} />
    <input
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
        <span class="avatar">{conversation.peer.display_name.slice(0, 1)}</span>
        <div class="conversation-copy">
          <div class="conversation-name">
            <strong>{conversation.peer.display_name}</strong>
            {#if conversation.last_message}
              <time>
                {time(conversation.last_message.created_at)}
              </time>
            {/if}
          </div>
          <span class="conversation-username">
            @{conversation.peer.username}
          </span>
          <div class="conversation-preview">
            <p>
              {sync.typingByConversation[conversation.id]
                ? '正在输入…'
                : (conversation.last_message?.body ?? '开始你们的第一段对话')}
            </p>
            {#if conversation.unread_count > 0}
              <span class="unread-badge">
                {conversation.unread_count > 99
                  ? '99+'
                  : conversation.unread_count}
              </span>
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
