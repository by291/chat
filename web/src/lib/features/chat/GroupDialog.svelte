<script lang="ts">
  import { Dialog } from 'bits-ui';
  import { Users, X } from '@lucide/svelte';
  import { goto } from '$app/navigation';

  import { createGroupApi } from './api';
  import type { ChatSync } from './sync.svelte';

  let { sync }: { sync: ChatSync } = $props();
  const requests = createGroupApi();
  let open = $state(false);
  let title = $state('');
  let token = $state('');
  let busy = $state(false);
  let error = $state('');

  async function submit(join: boolean) {
    if (busy) return;
    busy = true;
    error = '';

    try {
      const group = join
        ? await requests.accept(token.trim())
        : await requests.create(title);
      // The authenticated tree may have been replaced while the operation ran.
      if (!sync.alive) return;
      await sync.refreshSummaries();
      open = false;
      token = '';
      await goto(`/app/chats/${group.id}`);
    } catch (reason) {
      error = reason instanceof Error ? reason.message : '群聊操作失败';
    } finally {
      busy = false;
    }
  }
</script>

<Dialog.Root bind:open>
  <Dialog.Trigger
    class="icon-button"
    aria-label="创建或加入群聊"
    onclick={() => {
      error = '';
      token = '';
    }}
  >
    <Users size={20} />
  </Dialog.Trigger>
  <Dialog.Portal>
    <Dialog.Overlay class="dialog-overlay" />
    <Dialog.Content class="dialog-content">
      <Dialog.Close class="icon-button dialog-close" aria-label="关闭">
        <X size={20} />
      </Dialog.Close>
      <Dialog.Title class="dialog-title">一起聊聊</Dialog.Title>
      <Dialog.Description class="muted">
        创建一个私人群聊，再邀请你的伙伴加入。
      </Dialog.Description>
      <form
        onsubmit={(event) => {
          event.preventDefault();
          void submit(false);
        }}
      >
        <label for="group-title">群名称</label>
        <input
          id="group-title"
          bind:value={title}
          required
          maxlength="200"
          placeholder="例如：我们的项目"
        />
        <button class="button primary" disabled={busy || !title.trim()}>
          创建群聊
        </button>
      </form>
      <form
        class="group-join-form"
        onsubmit={(event) => {
          event.preventDefault();
          void submit(true);
        }}
      >
        <label for="group-token">已有群邀请</label>
        <input
          id="group-token"
          bind:value={token}
          autocomplete="off"
          placeholder="粘贴邀请口令"
        />
        <button class="button subtle" disabled={busy || !token.trim()}>
          加入群聊
        </button>
      </form>
      {#if error}<p class="form-error" role="alert">{error}</p>{/if}
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>
