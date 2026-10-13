<script lang="ts">
  import * as Avatar from '#lib/components/ui/avatar/index.js';
  import { Button } from '#lib/components/ui/button/index.js';

  import { untrack } from 'svelte';
  import { goto } from '$app/navigation';
  import { ArrowLeft, MessageCircle, ShieldCheck } from '@lucide/svelte';
  import { useChat } from './context.svelte';
  import ConversationList from './ConversationList.svelte';
  import MessageList from './MessageList.svelte';
  import Composer from './Composer.svelte';
  import GroupDetails from './GroupDetails.svelte';
  import ProfileAvatar from '../settings/ProfileAvatar.svelte';
  import { isGroup, conversationName } from './models';

  let {
    id = null,
  }: {
    id?: string | null;
  } = $props();

  const chat = useChat();

  $effect(() => {
    const sync = chat.sync;
    const selected = id;
    if (sync) {
      // Route selection drives this effect; cache changes inside select must not
      // trigger another selection and invalidate its in-flight snapshot.
      untrack(() => {
        if (selected) {
          void sync.select(selected);
        } else {
          sync.clearSelection();
        }
      });
    }
    return () => {
      if (sync && selected) {
        sync.setVisible(false);
        void sync.setTyping(selected, false);
      }
    };
  });
</script>

<svelte:head>
  <title>{conversationName(chat.sync?.current)} · Chat</title>
</svelte:head>
{#if chat.sync}
  {@const sync = chat.sync}
  <main class="chat-shell" class:has-conversation={!!id}>
    <ConversationList {sync} />
    <section class="chat-panel">
      {#if id}
        {@const current = sync.current}
        <header class="chat-heading">
          <Button
            variant="ghost"
            size="icon"
            class="mobile-back"
            aria-label="返回会话列表"
            onclick={() => goto('/app')}
          >
            <ArrowLeft size={21} />
          </Button>
          {#if current && !isGroup(current)}
            <ProfileAvatar
              userId={current.peer.id}
              name={current.peer.display_name}
            />
          {:else}
            <Avatar.Root>
              <Avatar.Fallback>
                {Array.from(conversationName(current))[0] ?? '…'}
              </Avatar.Fallback>
            </Avatar.Root>
          {/if}
          <div class="chat-person">
            <h2>{conversationName(current)}</h2>
            <p>
              {#if sync.typingByConversation[id]}
                <span class="typing-status">正在输入…</span>
              {:else if isGroup(current)}
                群聊 · {current.member_count} 位成员
              {:else if current}
                <span
                  class="online-dot"
                  class:is-offline={!sync.onlineByUser[current.peer.id]}
                ></span>
                {sync.onlineByUser[current.peer.id] === true
                  ? '在线'
                  : sync.onlineByUser[current.peer.id] === false
                    ? '离线'
                    : '状态暂不可用'}
                <span class="person-username">
                  @{current.peer.username}
                </span>
              {/if}
            </p>
          </div>
          {#if isGroup(current)}<GroupDetails {sync} />{/if}
        </header>
        {#if sync.error}
          <div class="sync-error" role="alert">
            <span>{sync.error}</span>
            <Button variant="link" size="sm" onclick={() => sync.reload()}>
              重新加载
            </Button>
          </div>
        {/if}
        {#key id}
          {#if current}
            <MessageList {sync} {id} userId={sync.userId} />
            <Composer {sync} {id} />
          {:else}
            <p class="message-placeholder">会话暂不可用，请返回会话列表。</p>
          {/if}
        {/key}
      {:else}
        <div class="welcome-chat">
          <span class="welcome-icon"><MessageCircle size={38} /></span>
          <span class="eyebrow">ROOM FOR YOUR PEOPLE</span>
          <h2>
            让日常的联系
            <br />
            多一点温度。
          </h2>
          <p>
            选一个聊天，或找到新的伙伴。
            <br />
            你们的对话，从这里开始。
          </p>
          <span class="welcome-foot">
            <ShieldCheck size={14} />只对会话中的成员开放
          </span>
        </div>
      {/if}
    </section>
  </main>
{:else}
  <main class="loading-page">正在准备聊天…</main>
{/if}
