<script lang="ts">
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
          <progress
            max="100"
            value={entry.progress}
            aria-label={`${entry.file.name} 上传进度`}
          ></progress>
          <span>{entry.progress}%</span>
        {/if}
      </div>
      {#if entry.error}
        <button
          type="button"
          class="button subtle compact"
          onclick={() => uploads.retry(entry.localId)}
        >
          重试
        </button>
      {/if}
      <button
        type="button"
        class="button subtle compact"
        onclick={() => uploads.cancel(entry.localId)}
      >
        取消
      </button>
    </li>
  {/each}
</ul>
