<script lang="ts">
  import { Button } from '#lib/components/ui/button/index.js';
  import { Label } from '#lib/components/ui/label/index.js';
  import { Textarea } from '#lib/components/ui/textarea/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';

  import type { ChatSync } from './sync.svelte';

  let { sync }: { sync: ChatSync } = $props();

  async function withdraw() {
    const target = sync.actions.withdrawingTarget;
    if (target) await sync.actions.withdraw(target);
    sync.actions.withdrawingId = null;
  }
</script>

<Dialog.Root
  open={!!sync.actions.editingId}
  onOpenChange={(open) => {
    if (!open) sync.actions.cancelEdit();
  }}
>
  <Dialog.Content closeLabel="关闭">
    <Dialog.Header>
      <Dialog.Title>编辑消息</Dialog.Title>
      <Dialog.Description>更新文字内容，附件保持不变。</Dialog.Description>
    </Dialog.Header>
    <form
      class="message-edit-form"
      onsubmit={(event) => {
        event.preventDefault();
        void sync.actions.saveEdit();
      }}
    >
      <Label for="edited-message">消息内容</Label>
      <Textarea
        id="edited-message"
        bind:value={sync.actions.editBody}
        rows={5}
      />
      <Dialog.Footer>
        <Button
          type="button"
          variant="outline"
          onclick={() => sync.actions.cancelEdit()}
        >
          取消
        </Button>
        <Button type="submit" disabled={sync.actions.busy || !sync.canSaveEdit}>
          保存修改
        </Button>
      </Dialog.Footer>
    </form>
  </Dialog.Content>
</Dialog.Root>

<Dialog.Root
  open={!!sync.actions.withdrawingId}
  onOpenChange={(open) => {
    if (!open) sync.actions.withdrawingId = null;
  }}
>
  <Dialog.Content closeLabel="关闭">
    <Dialog.Header>
      <Dialog.Title>撤回这条消息？</Dialog.Title>
      <Dialog.Description>
        会话中的所有人将看到撤回提示，消息内容和附件将隐藏。此操作无法恢复。
      </Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button
        type="button"
        variant="outline"
        onclick={() => (sync.actions.withdrawingId = null)}
      >
        取消
      </Button>
      <Button
        type="button"
        variant="destructive"
        disabled={sync.actions.busy}
        onclick={withdraw}
      >
        确认撤回
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
