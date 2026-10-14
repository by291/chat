<script lang="ts">
  import { Button } from '#lib/components/ui/button/index.js';

  import { onMount } from 'svelte';
  import { Bell, X } from '@lucide/svelte';
  import { goto } from '$app/navigation';
  import type { ChatSync } from './sync.svelte';

  let { sync }: { sync: ChatSync } = $props();

  function openConversation(id: string) {
    if (!sync.alive) return;
    // Navigation mounts the conversation composition before it selects history.
    // The alert center also lives on settings and group-invitation routes.
    void goto(`/app/chats/${id}`);
  }

  onMount(() => {
    sync.notifications.mount(openConversation);
  });
</script>

<div class="in-app-alerts" aria-live="polite" aria-label="新消息提醒">
  {#each sync.notifications.items as alert (alert.messageId)}
    <div class="in-app-alert" role="status">
      <Button
        type="button"
        variant="ghost"
        class="alert-open"
        onclick={() => {
          sync.notifications.dismiss(alert.messageId);
          openConversation(alert.conversationId);
        }}
      >
        <Bell size={16} />
        <span>{alert.title} · 新消息</span>
      </Button>
      <Button
        type="button"
        variant="ghost"
        size="icon-sm"
        aria-label="关闭提醒"
        onclick={() => sync.notifications.dismiss(alert.messageId)}
      >
        <X size={14} />
      </Button>
    </div>
  {/each}
</div>
