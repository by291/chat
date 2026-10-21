<script lang="ts">
  import { Ellipsis } from '@lucide/svelte';

  import { Button } from '#lib/components/ui/button/index.js';
  import * as DropdownMenu from '#lib/components/ui/dropdown-menu/index.js';

  import CreateGroupDialog from '../groups/CreateGroupDialog.svelte';
  import JoinGroupDialog from '../groups/JoinGroupDialog.svelte';
  import type { ChatSync } from '../sync.svelte';

  let { sync }: { sync: ChatSync } = $props();

  let trigger = $state<HTMLButtonElement | null>(null);
  let startOpen = $state(false);
  let joinOpen = $state(false);

  function restoreFocus(event: Event) {
    event.preventDefault();
    trigger?.focus();
  }
</script>

<DropdownMenu.Root>
  <DropdownMenu.Trigger aria-label="群聊操作" title="群聊操作">
    {#snippet child({ props })}
      <Button
        bind:ref={trigger}
        variant="ghost"
        size="icon"
        class="size-9"
        {...props}
      >
        <Ellipsis class="size-5" />
      </Button>
    {/snippet}
  </DropdownMenu.Trigger>
  <DropdownMenu.Content
    align="start"
    class="min-w-40"
    onCloseAutoFocus={(event) => {
      // A selected dialog owns focus until it closes; the menu must not reclaim it.
      if (startOpen || joinOpen) event.preventDefault();
    }}
  >
    <DropdownMenu.Item
      class="min-h-10 px-3"
      onSelect={() => (startOpen = true)}
    >
      创建群聊
    </DropdownMenu.Item>
    <DropdownMenu.Item class="min-h-10 px-3" onSelect={() => (joinOpen = true)}>
      加入群聊
    </DropdownMenu.Item>
  </DropdownMenu.Content>
</DropdownMenu.Root>

<CreateGroupDialog
  {sync}
  bind:open={startOpen}
  onCloseAutoFocus={restoreFocus}
/>
<JoinGroupDialog {sync} bind:open={joinOpen} onCloseAutoFocus={restoreFocus} />
