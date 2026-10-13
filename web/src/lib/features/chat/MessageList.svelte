<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import { Check, CheckCheck, ArrowDown, RotateCcw } from '@lucide/svelte';
  import type { ChatSync } from './sync.svelte';
  import { isGroup } from './models';
  import AttachmentList from '../media/AttachmentList.svelte';

  let {
    sync,
    id,
    userId,
  }: {
    sync: ChatSync;
    id: string;
    userId: string;
  } = $props();

  let scroller: HTMLDivElement;
  let bottom: HTMLDivElement;
  let following = $state(true);
  let renderGeneration = 0;
  let renderedId = '';
  const formatTime = (value: string) =>
    new Date(value).toLocaleTimeString('zh-CN', {
      hour: '2-digit',
      minute: '2-digit',
    });

  // A received/cached message is not read until its rendered bottom is inside
  // the focused visible viewport. Every DOM update invalidates this confirmation.
  function confirmRenderedVisibility() {
    if (!scroller || !bottom || sync.selectedId !== id || sync.loading) {
      sync.setVisible(false);
      return;
    }
    const viewport = scroller.getBoundingClientRect();
    const end = bottom.getBoundingClientRect();
    sync.setVisible(
      document.visibilityState === 'visible' &&
        document.hasFocus() &&
        end.top >= viewport.top &&
        end.bottom <= viewport.bottom + 2,
    );
  }

  function handleScroll() {
    following =
      scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 80;
    confirmRenderedVisibility();
  }

  function scrollToLatest() {
    following = true;
    scroller.scrollTop = scroller.scrollHeight;
    confirmRenderedVisibility();
  }

  async function loadOlderPreservingScroll() {
    const height = scroller.scrollHeight;
    const top = scroller.scrollTop;
    await sync.loadOlder();
    await tick();
    // Anchor the same visible history after prepending older records.
    scroller.scrollTop = top + scroller.scrollHeight - height;
    confirmRenderedVisibility();
  }

  async function mediaLayoutChanged() {
    sync.setVisible(false);
    await tick();
    if (following) scroller.scrollTop = scroller.scrollHeight;
    confirmRenderedVisibility();
  }

  $effect(() => {
    sync.messages.length;
    sync.currentPending.length;
    sync.loading;
    // Replay can advance without changing row count, so the applied cursor is
    // also a dependency for a new rendered-read confirmation.
    sync.appliedSeq;
    const conversation = id;
    const generation = ++renderGeneration;
    sync.setVisible(false);
    // Scrolling alone must not schedule another render/auto-scroll cycle.
    const wasFollowing = untrack(() => following);
    const changed = renderedId !== conversation;
    renderedId = conversation;
    void tick().then(() => {
      if (
        generation !== renderGeneration ||
        sync.selectedId !== conversation ||
        !scroller
      ) {
        return;
      }
      if (changed || wasFollowing) {
        scroller.scrollTop = scroller.scrollHeight;
      }
      confirmRenderedVisibility();
    });
  });

  onMount(() => {
    const observer = new ResizeObserver(() => {
      if (following) {
        scroller.scrollTop = scroller.scrollHeight;
      }
      confirmRenderedVisibility();
    });
    observer.observe(scroller);
    window.addEventListener('focus', confirmRenderedVisibility);
    window.addEventListener('blur', confirmRenderedVisibility);
    document.addEventListener('visibilitychange', confirmRenderedVisibility);
    return () => {
      observer.disconnect();
      window.removeEventListener('focus', confirmRenderedVisibility);
      window.removeEventListener('blur', confirmRenderedVisibility);
      document.removeEventListener(
        'visibilitychange',
        confirmRenderedVisibility,
      );
      sync.setVisible(false);
    };
  });
</script>

<div class="message-area">
  <div class="message-scroll" bind:this={scroller} onscroll={handleScroll}>
    <div
      class="message-inner"
      role="log"
      aria-label="聊天消息"
      aria-live="polite"
    >
      {#if sync.hasOlder}
        <button
          class="history-button"
          disabled={sync.loadingOlder}
          onclick={loadOlderPreservingScroll}
        >
          {sync.loadingOlder ? '正在加载…' : '加载更早的消息'}
        </button>
      {/if}
      {#if sync.loading}
        <p class="message-placeholder">正在加载聊天记录…</p>
      {:else if !sync.messages.length && !sync.currentPending.length}
        <div class="first-message">
          <span>一段新对话</span>
          <p>从一句简单的问候开始吧。</p>
        </div>
      {/if}
      {#each sync.messages as message (message.id)}
        <article class="message-row" class:own={message.sender_id === userId}>
          <div class="message-bubble">
            {#if isGroup(sync.current)}
              <strong class="message-sender">
                {message.sender_name ?? sync.senderName(message.sender_id)}
              </strong>
            {/if}
            {#if message.body}<p>{message.body}</p>{/if}
            {#each message.attachments ?? [] as attachment (attachment.id)}
              <AttachmentList {attachment} onLayout={mediaLayoutChanged} />
            {/each}
            <div class="message-meta">
              <time datetime={message.created_at}>
                {formatTime(message.created_at)}
              </time>
              {#if message.sender_id === userId}
                {#if isGroup(sync.current)}
                  {#if sync.groupMembers.length}
                    <span
                      aria-label={`${sync.readCount(message.created_seq)} 人已读`}
                    >
                      {sync.readCount(message.created_seq)} 人已读
                    </span>
                  {/if}
                {:else if BigInt(sync.current?.peer_read_seq ?? '0') >= BigInt(message.created_seq)}
                  <span aria-label="已读">
                    <CheckCheck size={13} />
                  </span>
                {:else}
                  <span aria-label="已保存">
                    <Check size={13} />
                  </span>
                {/if}
              {/if}
            </div>
          </div>
        </article>
      {/each}
      {#each sync.currentPending as message (message.client_message_id)}
        <article class="message-row own">
          <div
            class="message-bubble pending"
            class:failed={message.status === 'failed'}
          >
            <p>{message.body}</p>
            {#if message.attachment_ids.length}<small>
                {message.attachment_ids.length} 个附件
              </small>{/if}
            <div class="message-meta">
              {#if message.status === 'failed'}
                <button onclick={() => sync.retry(message.client_message_id)}>
                  <RotateCcw size={12} />发送失败，重试
                </button>
              {:else}
                <span>正在发送…</span>
              {/if}
            </div>
          </div>
        </article>
      {/each}
      <div class="message-bottom" bind:this={bottom}></div>
    </div>
  </div>
  {#if !following}
    <button class="jump-latest" onclick={scrollToLatest}>
      <ArrowDown size={15} />查看最新消息
    </button>
  {/if}
</div>
