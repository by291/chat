<script lang="ts">
  import { Button } from '#lib/components/ui/button/index.js';
  import { Progress } from '#lib/components/ui/progress/index.js';

  import type { MediaUploads, UploadDraft } from './uploads.svelte';

  let { uploads, entries }: { uploads: MediaUploads; entries: UploadDraft[] } =
    $props();
</script>

<ul class="upload-list" aria-label="待发送附件">
  {#each entries as entry (entry.localId)}
    <li>
      <div class="upload-description">
        <strong>{entry.file.name}</strong>
        {#if entry.error}
          <span class="form-error" role="alert">{entry.error}</span>
        {:else if uploads.metadata(entry)?.state === 'ready'}
          <span>已就绪</span>
        {:else if uploads.metadata(entry)?.state === 'processing'}
          <span role="status">正在处理…</span>
        {:else}
          <Progress
            max={100}
            value={entry.progress}
            aria-label={`${entry.file.name} 上传进度`}
          />
          <span>{entry.progress}%</span>
        {/if}
      </div>
      {#if entry.error}
        <Button
          type="button"
          variant="secondary"
          size="sm"
          onclick={() => uploads.retry(entry.localId)}
        >
          重试
        </Button>
      {/if}
      <Button
        type="button"
        variant="secondary"
        size="sm"
        onclick={() => uploads.cancel(entry.localId)}
      >
        取消
      </Button>
    </li>
  {/each}
</ul>
