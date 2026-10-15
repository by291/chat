<script lang="ts">
  import { Users } from '@lucide/svelte';
  import { goto } from '$app/navigation';

  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Label } from '#lib/components/ui/label/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';

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
    aria-label="创建或加入群聊"
    onclick={() => {
      error = '';
      token = '';
    }}
  >
    {#snippet child({ props })}
      <Button variant="ghost" size="icon" {...props}>
        <Users size={20} />
      </Button>
    {/snippet}
  </Dialog.Trigger>
  <Dialog.Content closeLabel="关闭">
    <Dialog.Title>一起聊聊</Dialog.Title>
    <Dialog.Description>
      创建一个私人群聊，再邀请你的伙伴加入。
    </Dialog.Description>
    <form
      onsubmit={(event) => {
        event.preventDefault();
        void submit(false);
      }}
    >
      <Label class="form-label" for="group-title">群名称</Label>
      <Input
        id="group-title"
        bind:value={title}
        required
        maxlength={200}
        placeholder="例如：我们的项目"
      />
      <Button type="submit" disabled={busy || !title.trim()}>创建群聊</Button>
    </form>
    <form
      class="group-join-form"
      onsubmit={(event) => {
        event.preventDefault();
        void submit(true);
      }}
    >
      <Label class="form-label" for="group-token">已有群邀请</Label>
      <Input
        id="group-token"
        bind:value={token}
        autocomplete="off"
        placeholder="粘贴邀请口令"
      />
      <Button
        type="submit"
        variant="secondary"
        disabled={busy || !token.trim()}
      >
        加入群聊
      </Button>
    </form>
    {#if error}
      <p class="form-error" role="alert">{error}</p>
    {/if}
  </Dialog.Content>
</Dialog.Root>
