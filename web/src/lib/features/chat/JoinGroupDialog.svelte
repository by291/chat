<script lang="ts">
  import { onDestroy } from 'svelte';
  import { goto } from '$app/navigation';

  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Label } from '#lib/components/ui/label/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';

  import { createGroupApi } from './api';
  import type { ChatSync } from './sync.svelte';

  let {
    sync,
    open = $bindable(false),
    onCloseAutoFocus,
  }: {
    sync: ChatSync;
    open: boolean;
    onCloseAutoFocus: (event: Event) => void;
  } = $props();

  const requests = createGroupApi();
  const lifetime = new AbortController();
  let token = $state('');
  let busy = $state(false);
  let error = $state('');

  async function join() {
    if (busy || !token.trim()) return;
    busy = true;
    error = '';
    const owner = sync;

    try {
      const group = await requests.accept(token, lifetime.signal);
      // A late join belongs to both the initiating page and authenticated owner.
      if (lifetime.signal.aborted || !owner.alive || sync !== owner) return;
      await owner.refreshSummaries();
      if (lifetime.signal.aborted || !owner.alive || sync !== owner) return;
      open = false;
      token = '';
      await goto(`/app/chats/${group.id}`);
    } catch (reason) {
      if (!lifetime.signal.aborted)
        error = reason instanceof Error ? reason.message : '加入群聊失败';
    } finally {
      busy = false;
    }
  }

  $effect(() => {
    if (open) {
      token = '';
      error = '';
    }
  });

  onDestroy(() => lifetime.abort());
</script>

<Dialog.Root bind:open>
  <Dialog.Content
    {onCloseAutoFocus}
    closeLabel="关闭"
    showCloseButton={!busy}
    onEscapeKeydown={(event) => {
      if (busy) event.preventDefault();
    }}
    onInteractOutside={(event) => {
      if (busy) event.preventDefault();
    }}
  >
    <Dialog.Header>
      <Dialog.Title>加入群聊</Dialog.Title>
      <Dialog.Description>粘贴朋友分享的群邀请链接或口令。</Dialog.Description>
    </Dialog.Header>
    <form
      class="group-join-form"
      onsubmit={(event) => {
        event.preventDefault();
        void join();
      }}
    >
      <Label for="group-token">邀请链接或口令</Label>
      <Input
        id="group-token"
        bind:value={token}
        autocomplete="off"
        placeholder="粘贴群邀请链接或口令"
        required
        disabled={busy}
      />
      <Button
        type="submit"
        class="mt-2 w-full"
        disabled={busy || !token.trim()}
      >
        {busy ? '正在加入…' : '加入群聊'}
      </Button>
    </form>
    {#if error}
      <p class="form-error" role="alert">{error}</p>
    {/if}
  </Dialog.Content>
</Dialog.Root>
