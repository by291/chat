<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import { beforeNavigate, afterNavigate } from '$app/navigation';
  import { ArrowDown, RotateCcw } from '@lucide/svelte';

  import { Button } from '#lib/components/ui/button/index.js';

  import { useChat } from './context.svelte';
  import { membershipId } from './models';

  import type { ChatSync } from './sync.svelte';
  import MessageBubble from './MessageBubble.svelte';

  const FOLLOW_LATEST_TOLERANCE_PX = 80;
  const VISIBLE_BOTTOM_TOLERANCE_PX = 2;

  let {
    sync,
    id,
    userId,
  }: {
    sync: ChatSync;
    id: string;
    userId: string;
  } = $props();

  const chat = useChat();
  const episode = untrack(() => membershipId(sync.current));
  let scroller: HTMLDivElement;
  let bottom: HTMLDivElement;
  let following = $state(true);
  let renderGeneration = 0;
  let renderedId = '';
  let renderedTarget: string | null = null;
  let navigating = false;

  // SvelteKit resets scrolling during navigation. Capture before that reset and
  // ignore teardown scroll events, which no longer represent the reader's place.
  beforeNavigate(() => {
    rememberPosition();
    navigating = true;
  });
  afterNavigate(() => {
    navigating = false;
  });

  // A received/cached message is not read until its rendered bottom is inside
  // the focused visible viewport. Every DOM update invalidates this confirmation.
  function confirmRenderedVisibility() {
    const focused =
      document.visibilityState === 'visible' &&
      document.hasFocus() &&
      sync.selectedId === id;
    sync.setFocused(focused);
    if (!scroller || !bottom || sync.selectedId !== id || sync.loading) {
      sync.setVisible(false);
      return;
    }

    const viewport = scroller.getBoundingClientRect();
    const end = bottom.getBoundingClientRect();
    sync.setVisible(
      focused &&
        end.top >= viewport.top &&
        end.bottom <= viewport.bottom + VISIBLE_BOTTOM_TOLERANCE_PX,
    );
  }

  function rememberPosition() {
    if (
      !scroller ||
      !scroller.isConnected ||
      navigating ||
      sync.loading ||
      sync.selectedId !== id ||
      sync.history.contextual
    )
      return;
    const top = scroller.getBoundingClientRect().top;
    const anchor = Array.from(
      scroller.querySelectorAll<HTMLElement>('[data-message-id]'),
    ).find((element) => element.getBoundingClientRect().bottom > top);
    chat.readingPositions.set(id, {
      membershipId: episode,
      anchorId: anchor?.dataset.messageId,
      offset: anchor ? anchor.getBoundingClientRect().top - top : 0,
      scrollTop: scroller.scrollTop,
      following,
    });
  }

  function restorePosition() {
    const saved = chat.readingPositions.get(id);
    if (!saved || saved.membershipId !== episode || saved.following) {
      following = true;
      scroller.scrollTop = scroller.scrollHeight;
      return;
    }
    following = false;
    const anchor = saved.anchorId
      ? document.getElementById(`message-${saved.anchorId}`)
      : null;
    scroller.scrollTop = anchor
      ? scroller.scrollTop +
        anchor.getBoundingClientRect().top -
        scroller.getBoundingClientRect().top -
        saved.offset
      : saved.scrollTop;
  }

  function handleScroll() {
    following =
      scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight <
      FOLLOW_LATEST_TOLERANCE_PX;
    rememberPosition();
    confirmRenderedVisibility();
  }

  async function scrollToLatest() {
    if (sync.history.contextual) await sync.history.latest();
    await tick();

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
    if (following && !sync.history.contextual) {
      scroller.scrollTop = scroller.scrollHeight;
    }
    confirmRenderedVisibility();
  }

  $effect(() => {
    sync.messages;
    sync.currentPending.length;
    sync.loading;
    // Replay can advance without changing row count, so the applied cursor is
    // also a dependency for a new rendered-read confirmation.
    sync.appliedSeq;
    const contextual = sync.history.contextual;
    const target = sync.history.targetId;
    const jump = target !== renderedTarget;
    renderedTarget = target;
    const conversation = id;
    const generation = ++renderGeneration;
    sync.setVisible(false);
    // Scrolling alone must not schedule another render/auto-scroll cycle.
    const wasFollowing = untrack(() => following);
    const changed = renderedId !== conversation;
    void tick().then(() => {
      if (
        generation !== renderGeneration ||
        sync.selectedId !== conversation ||
        !scroller ||
        sync.loading
      ) {
        return;
      }
      if (contextual) {
        following = false;
        if (target && jump) {
          const element = document.getElementById(`message-${target}`);
          element?.scrollIntoView({ block: 'center' });
          element?.focus({ preventScroll: true });
        }
      } else if (changed) {
        // Restore only after the canonical history is ready. An anchor keeps the
        // same message visible even if new records arrived while this route slept.
        untrack(restorePosition);
      } else if (wasFollowing) {
        scroller.scrollTop = scroller.scrollHeight;
      }
      renderedId = conversation;
      confirmRenderedVisibility();
    });
  });

  onMount(() => {
    const observer = new ResizeObserver(() => {
      if (renderedId && following && !sync.history.contextual) {
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
      sync.setFocused(false);
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
        <Button
          variant="ghost"
          size="sm"
          class="history-button"
          disabled={sync.loadingOlder}
          onclick={loadOlderPreservingScroll}
        >
          {sync.loadingOlder ? '正在加载…' : '加载更早的消息'}
        </Button>
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
        <article
          id={`message-${message.id}`}
          data-message-id={message.id}
          tabindex="-1"
          class="message-row"
          class:own={message.sender_id === userId}
          class:context-target={sync.history.contextual &&
            sync.history.targetId === message.id}
        >
          <MessageBubble {sync} {message} onLayout={mediaLayoutChanged} />
        </article>
      {/each}
      {#each sync.currentPending as message (message.client_message_id)}
        <article class="message-row own">
          <div
            class="message-bubble pending"
            class:failed={message.status === 'failed'}
          >
            <p>{message.body}</p>
            {#if message.attachment_ids.length}
              <small>
                {message.attachment_ids.length} 个附件
              </small>
            {/if}
            <div class="message-meta">
              {#if message.status === 'failed'}
                <Button
                  variant="link"
                  size="xs"
                  class="h-auto p-0 text-destructive"
                  onclick={() => sync.retry(message.client_message_id)}
                >
                  <RotateCcw size={12} />发送失败，重试
                </Button>
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
  {#if !following || sync.history.contextual}
    <Button variant="secondary" class="jump-latest" onclick={scrollToLatest}>
      <ArrowDown size={15} />{sync.history.contextual
        ? '返回最新消息'
        : '查看最新消息'}
    </Button>
  {/if}
</div>
