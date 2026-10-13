<script lang="ts">
  import { Dialog } from 'bits-ui';
  import { Plus, X, ArrowUpRight } from '@lucide/svelte';
  import { goto } from '$app/navigation';
  import { findUser, startDirectConversation } from './api';
  import type { components } from '#lib/api-client';
  import type { ChatSync } from './sync.svelte';

  let {
    sync,
  }: {
    sync: ChatSync;
  } = $props();

  let open = $state(false);
  let username = $state('');
  let error = $state('');
  let busy = $state(false);
  let peer = $state<components['schemas']['UserView'] | undefined>();

  async function find() {
    busy = true;
    error = '';
    peer = undefined;

    try {
      peer = await findUser(username);
    } catch (reason) {
      error = reason instanceof Error ? reason.message : '没有找到这个用户';
    } finally {
      busy = false;
    }
  }

  async function start() {
    if (!peer) {
      return;
    }
    busy = true;
    error = '';

    try {
      const conversation = await startDirectConversation(peer.id);
      await sync.refreshSummaries();
      open = false;
      await goto(`/app/chats/${conversation.id}`);
    } catch (reason) {
      error = reason instanceof Error ? reason.message : '暂时无法创建聊天';
    } finally {
      busy = false;
    }
  }
</script>

<Dialog.Root bind:open>
  <Dialog.Trigger
    class="icon-button new-chat"
    aria-label="发起聊天"
    onclick={() => {
      username = '';
      peer = undefined;
      error = '';
    }}
  >
    <Plus size={21} />
  </Dialog.Trigger>

  <Dialog.Portal>
    <Dialog.Overlay class="dialog-overlay" />

    <Dialog.Content class="dialog-content">
      <Dialog.Close class="icon-button dialog-close" aria-label="关闭">
        <X size={20} />
      </Dialog.Close>
      <span class="eyebrow">A NEW CONVERSATION</span>
      <Dialog.Title class="dialog-title">从一句你好开始</Dialog.Title>

      <Dialog.Description class="muted">
        通过用户名找到伙伴，开始一对一聊天。
      </Dialog.Description>
      <form
        onsubmit={(e) => {
          e.preventDefault();
          void find();
        }}
      >
        <label for="peer-username">对方的用户名</label>
        <input
          id="peer-username"
          bind:value={username}
          oninput={() => (peer = undefined)}
          placeholder="输入用户名，无需 @"
          autocomplete="off"
          required
        />
        <button
          class="button subtle find-button"
          disabled={busy || !username.trim()}
        >
          查找用户
        </button>
      </form>
      {#if peer}
        <div class="found-user">
          <span class="avatar">{peer.display_name.slice(0, 1)}</span>
          <div>
            <strong>{peer.display_name}</strong>
            <p>@{peer.username}</p>
          </div>
          <button class="button primary" disabled={busy} onclick={start}>
            开始聊天<ArrowUpRight size={16} />
          </button>
        </div>
      {/if}
      {#if error}
        <p class="form-error" role="alert">
          {error}
        </p>
      {/if}
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>
