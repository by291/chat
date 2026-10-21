<script lang="ts">
  import { Button } from '#lib/components/ui/button/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';

  import { useContacts } from '../context.svelte';
  import type { User } from '../api';

  let { removing = $bindable() }: { removing?: User } = $props();

  const contacts = useContacts();
</script>

<Dialog.Root
  open={!!removing}
  onOpenChange={(open) => {
    if (!open) removing = undefined;
  }}
>
  <Dialog.Content closeLabel="关闭">
    <Dialog.Title>删除好友</Dialog.Title>
    <Dialog.Description>
      删除 {removing?.display_name} 后将无法继续发送消息，已有聊天记录仍会保留。
    </Dialog.Description>
    <Button
      variant="destructive"
      disabled={contacts.owner?.busy}
      onclick={async () => {
        if (removing && (await contacts.owner?.remove(removing.id))) {
          removing = undefined;
        }
      }}
    >
      确认删除
    </Button>
    {#if contacts.owner?.mutationError}
      <p role="alert" class="form-error">
        {contacts.owner.mutationError}
      </p>
    {/if}
  </Dialog.Content>
</Dialog.Root>
