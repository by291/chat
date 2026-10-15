<script lang="ts">
  import { Button } from '#lib/components/ui/button/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';

  import { pwa } from './lifecycle.svelte';

  let {
    hasDraft = false,
    hasUpload = false,
    hasSubmission = false,
  }: {
    hasDraft?: boolean;
    hasUpload?: boolean;
    hasSubmission?: boolean;
  } = $props();
</script>

{#if pwa.updateAvailable}
  <aside class="pwa-update" role="status" aria-label="应用更新">
    <div>
      <strong>发现新版本</strong>
      <p>重新加载会中断进行中的操作并丢失未保存内容，请先完成或保存。</p>
      {#if pwa.error}
        <p>{pwa.error}</p>
      {/if}
    </div>
    <Button
      type="button"
      disabled={pwa.updatePending}
      onclick={() => pwa.requestUpdate({ hasDraft, hasUpload, hasSubmission })}
    >
      {pwa.updatePending ? '正在更新…' : '更新并重新加载'}
    </Button>
  </aside>
{/if}
<Dialog.Root
  open={pwa.confirmationRequired}
  onOpenChange={(open) => {
    if (!open) pwa.cancelUpdate();
  }}
>
  <Dialog.Content closeLabel="取消更新">
    <Dialog.Header>
      <Dialog.Title>确认更新？</Dialog.Title>
      <Dialog.Description>
        仍有草稿、附件或进行中的提交，继续会丢失当前页面的未保存内容。
      </Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button
        type="button"
        variant="outline"
        onclick={() => pwa.cancelUpdate()}
      >
        继续当前版本
      </Button>
      <Button type="button" onclick={() => pwa.confirmUpdate()}>
        确认更新并重新加载
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<style>
  .pwa-update {
    position: fixed;
    inset-inline: 1rem;
    bottom: 1rem;
    z-index: 40;
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 1rem;
    max-width: 38rem;
    margin-inline: auto;
    padding: 1rem;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--card);
    color: var(--card-foreground);
    box-shadow: 0 8px 24px rgb(0 0 0 / 12%);
  }
  .pwa-update div {
    flex: 1 1 16rem;
  }
  .pwa-update p {
    margin-block: 0.35rem 0;
    font-size: 0.85rem;
  }
</style>
