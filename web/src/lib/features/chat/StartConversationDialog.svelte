<script lang="ts">
  import { Plus, ArrowUpRight } from '@lucide/svelte';
  import { goto } from '$app/navigation';

  import * as Avatar from '#lib/components/ui/avatar/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Label } from '#lib/components/ui/label/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';
  import type { components } from '#lib/api-client';

  import { findUser, startDirectConversation } from './api';
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
    aria-label="发起聊天"
    onclick={() => {
      username = '';
      peer = undefined;
      error = '';
    }}
  >
    {#snippet child({ props })}
      <Button variant="ghost" size="icon" class="new-chat" {...props}>
        <Plus size={21} />
      </Button>
    {/snippet}
  </Dialog.Trigger>

  <Dialog.Content closeLabel="关闭">
    <span class="eyebrow">A NEW CONVERSATION</span>
    <Dialog.Title>从一句你好开始</Dialog.Title>

    <Dialog.Description>
      通过用户名找到伙伴，开始一对一聊天。
    </Dialog.Description>
    <form
      onsubmit={(e) => {
        e.preventDefault();
        void find();
      }}
    >
      <Label class="form-label" for="peer-username">对方的用户名</Label>
      <Input
        id="peer-username"
        bind:value={username}
        oninput={() => (peer = undefined)}
        placeholder="输入用户名，无需 @"
        autocomplete="off"
        required
      />
      <Button
        type="submit"
        variant="secondary"
        class="find-button"
        disabled={busy || !username.trim()}
      >
        查找用户
      </Button>
    </form>
    {#if peer}
      <div class="found-user">
        <Avatar.Root>
          <Avatar.Fallback>{peer.display_name.slice(0, 1)}</Avatar.Fallback>
        </Avatar.Root>
        <div class="found-user-copy">
          <strong>{peer.display_name}</strong>
          <p>@{peer.username}</p>
        </div>
        <Button disabled={busy} onclick={start}>
          开始聊天<ArrowUpRight size={16} />
        </Button>
      </div>
    {/if}
    {#if error}
      <p class="form-error" role="alert">
        {error}
      </p>
    {/if}
  </Dialog.Content>
</Dialog.Root>
