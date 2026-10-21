<script lang="ts">
  import { Bell } from '@lucide/svelte';

  import { Button } from '#lib/components/ui/button/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';

  import type { ChatSync } from '../sync.svelte';

  let { sync }: { sync: ChatSync } = $props();
</script>

<Dialog.Root>
  <Dialog.Trigger aria-label="通知设置">
    {#snippet child({ props })}
      <Button variant="ghost" size="icon" class="size-9" {...props}>
        <Bell class="size-5" />
      </Button>
    {/snippet}
  </Dialog.Trigger>
  <Dialog.Content closeLabel="关闭">
    <Dialog.Header>
      <Dialog.Title>通知设置</Dialog.Title>
      <Dialog.Description>
        仅在打开此页面时接收提醒。静音会话和正在查看的会话不会打扰你。
      </Dialog.Description>
    </Dialog.Header>
    <div class="notification-options">
      <p>页面内提醒已开启</p>
      <Button
        type="button"
        variant="outline"
        aria-pressed={sync.notifications.sound}
        onclick={() => sync.notifications.toggleSound()}
      >
        {sync.notifications.sound ? '关闭提示音' : '开启提示音'}
      </Button>
      <Button
        type="button"
        variant="outline"
        aria-pressed={sync.notifications.system}
        onclick={() => sync.notifications.toggleSystem()}
      >
        {sync.notifications.system ? '关闭系统通知' : '开启系统通知'}
      </Button>
      {#if sync.notifications.feedback}
        <p role="status">
          {sync.notifications.feedback}
        </p>
      {/if}
    </div>
  </Dialog.Content>
</Dialog.Root>
