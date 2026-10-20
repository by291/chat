<script lang="ts">
  import { onDestroy, untrack } from 'svelte';

  import { Button } from '#lib/components/ui/button/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';
  import { createGroupApi } from '#lib/features/chat/api.ts';
  import type { ChatSync } from '#lib/features/chat/sync.svelte.ts';
  import { membershipId } from '#lib/features/chat/models.ts';

  import FriendPicker from './FriendPicker.svelte';
  import type { User } from './api';

  let { sync, open = $bindable(false) }: { sync: ChatSync; open: boolean } =
    $props();
  let selected = $state<User[]>([]);
  let busy = $state(false);
  let error = $state('');
  const api = createGroupApi();
  const lifetime = new AbortController();
  const excludedIds = $derived(
    sync.groupMembers.map((member) => member.user.id),
  );

  async function add() {
    const id = sync.current?.id;
    const membership = membershipId(sync.current);
    if (!id || !membership || busy || !selected.length) return;
    busy = true;
    error = '';
    try {
      await api.addMembers(
        id,
        membership,
        selected.map((user) => user.id),
        lifetime.signal,
      );
      if (lifetime.signal.aborted) return;
      await Promise.all([sync.refreshSummaries(), sync.refreshMembers()]);
      if (!lifetime.signal.aborted) open = false;
    } catch (reason) {
      if (!lifetime.signal.aborted)
        error = reason instanceof Error ? reason.message : '添加成员失败';
    } finally {
      if (!lifetime.signal.aborted) busy = false;
    }
  }
  $effect(() => {
    if (open)
      untrack(() => {
        selected = [];
        error = '';
      });
  });
  onDestroy(() => lifetime.abort());
</script>

<Dialog.Root bind:open>
  <Dialog.Content
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
      <Dialog.Title>添加群成员</Dialog.Title>
      <Dialog.Description>
        选择好友直接加入群聊，新成员只能查看加入后的消息。
      </Dialog.Description>
    </Dialog.Header>
    <form
      class="new-conversation-form"
      onsubmit={(event) => {
        event.preventDefault();
        void add();
      }}
    >
      <FriendPicker bind:selected {excludedIds} disabled={busy} />
      {#if error}<p class="form-error" role="alert">{error}</p>{/if}
      <Button type="submit" disabled={busy || !selected.length}>
        {busy ? '正在添加…' : '添加成员'}
      </Button>
    </form>
  </Dialog.Content>
</Dialog.Root>
