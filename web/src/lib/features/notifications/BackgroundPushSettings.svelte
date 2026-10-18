<script lang="ts">
  import * as Card from '#lib/components/ui/card/index.js';
  import { Button } from '#lib/components/ui/button/index.js';

  import { useBackgroundPush } from './context.svelte';

  const push = useBackgroundPush();
  const cannotEnable = $derived(
    !push.owner?.supported ||
      !push.owner.configured ||
      push.owner.permission === 'denied',
  );
</script>

<Card.Root class="settings-card gap-0 p-6" role="region" aria-label="后台推送">
  <div class="section-heading">
    <h3>后台推送</h3>
    <p class="muted">
      关闭页面后也可收到通用的新消息提醒。页面内的声音和提醒另行设置。
    </p>
  </div>
  {#if push.owner}
    {#if !push.owner.configured && !push.owner.pending}
      <p class="muted">服务器尚未启用后台推送。</p>
    {:else if !push.owner.supported && !push.owner.pending}
      <p class="muted">
        当前环境不支持后台推送。iPhone／iPad 需先添加到主屏幕，再从主屏幕打开。
      </p>
    {:else if push.owner.permission === 'denied'}
      <p class="muted">通知权限已被拒绝，请先在浏览器或系统设置中允许通知。</p>
    {/if}
    <Button
      type="button"
      disabled={push.owner.pending || (!push.owner.enabled && cannotEnable)}
      onclick={() =>
        push.owner?.enabled ? push.owner.disable() : push.owner?.enable()}
    >
      {push.owner.pending
        ? '正在设置…'
        : push.owner.enabled
          ? '关闭后台推送'
          : '启用后台推送'}
    </Button>
    {#if push.owner.error}
      <p class="error" role="alert">{push.owner.error}</p>
    {/if}
  {/if}
</Card.Root>
