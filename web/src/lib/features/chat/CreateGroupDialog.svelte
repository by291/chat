<script lang="ts">
  import { onDestroy, untrack } from 'svelte';
  import { goto } from '$app/navigation';

  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Label } from '#lib/components/ui/label/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';
  import type { components } from '#lib/api-client';

  import { createGroupApi } from './api';
  import FriendPicker from '#lib/features/contacts/FriendPicker.svelte';
  import type { ChatSync } from './sync.svelte';

  type User = components['schemas']['UserView'];

  let {
    sync,
    open = $bindable(false),
    onCloseAutoFocus,
  }: {
    sync: ChatSync;
    open: boolean;
    onCloseAutoFocus: (event: Event) => void;
  } = $props();

  let selected = $state<User[]>([]);
  let error = $state('');
  let busy = $state(false);
  const lifetime = new AbortController();
  const groups = createGroupApi();
  let groupTitle = $state('');
  let createdId = $state('');

  const defaultTitle = $derived(
    Array.from(selected.map((user) => user.display_name).join('、'))
      .slice(0, 100)
      .join(''),
  );

  const selectedIds = $derived(selected.map((user) => user.id));
  function reset() {
    groupTitle = '';
    createdId = '';
    selected = [];
    error = '';
  }

  async function create() {
    if (busy || !selected.length) return;
    busy = true;
    error = '';
    const owner = sync;

    try {
      // Retain a successful creation if refreshing/navigation fails, so retry
      // finishes opening that group instead of creating it again.
      if (!createdId) {
        const conversation = await groups.create(
          groupTitle.trim() || defaultTitle,
          selectedIds,
          lifetime.signal,
        );
        if (lifetime.signal.aborted || !owner.alive || sync !== owner) return;
        createdId = conversation.id;
      }
      await owner.refreshSummaries();
      if (lifetime.signal.aborted || !owner.alive || sync !== owner) return;
      await goto(`/app/chats/${createdId}`);
      open = false;
    } catch (reason) {
      if (!lifetime.signal.aborted) {
        error =
          reason instanceof Error ? reason.message : '创建群聊失败，请重试';
      }
    } finally {
      if (!lifetime.signal.aborted) busy = false;
    }
  }

  $effect(() => {
    // Opening owns draft reset; draft edits must not subscribe this lifecycle.
    if (open) untrack(reset);
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
      <Dialog.Title>创建群聊</Dialog.Title>
      <Dialog.Description>选择好友，一起加入群聊。</Dialog.Description>
    </Dialog.Header>
    <form
      class="new-conversation-form"
      onsubmit={(event) => {
        event.preventDefault();
        void create();
      }}
    >
      <FriendPicker bind:selected disabled={busy || !!createdId} />
      <Label for="new-group-title">群名称（选填）</Label>
      <Input
        id="new-group-title"
        bind:value={groupTitle}
        maxlength={100}
        placeholder={defaultTitle}
        disabled={busy || !!createdId}
      />
      {#if error}
        <p class="form-error" role="alert">{error}</p>
      {/if}
      <Button
        type="submit"
        class="mt-2 w-full"
        disabled={busy || !selected.length}
      >
        {busy ? '正在创建…' : createdId ? '打开对话' : '创建群聊'}
      </Button>
    </form>
  </Dialog.Content>
</Dialog.Root>
