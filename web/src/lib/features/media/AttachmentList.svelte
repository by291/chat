<script lang="ts">
  import { tick, onDestroy } from 'svelte';

  import { Button } from '#lib/components/ui/button/index.js';

  import { createMediaApi, downloadBlob } from './api';
  import type { Attachment } from './models';

  const PREVIEW_REFRESH_INTERVAL_MS = 45_000;
  const DOWNLOAD_URL_RELEASE_DELAY_MS = 1_000;

  let {
    attachment,
    onLayout = () => {},
  }: { attachment: Attachment; onLayout?: () => void } = $props();

  const requests = createMediaApi();
  let root: HTMLDivElement;
  let preview = $state('');
  let error = $state('');
  let busy = $state(false);
  let visible = $state(false);
  let downloadController: AbortController | undefined;
  const image = $derived(!!attachment.width && !!attachment.height);
  const scale = $derived(
    Math.min(
      1,
      320 / (attachment.width ?? 320),
      512 / (attachment.height ?? 512),
    ),
  );
  const width = $derived(
    Math.max(1, Math.round((attachment.width ?? 320) * scale)),
  );
  const height = $derived(
    image ? Math.max(1, Math.round(attachment.height! * scale)) : 0,
  );

  async function download() {
    if (busy) return;

    busy = true;
    error = '';
    const controller = new AbortController();
    downloadController = controller;

    try {
      const grant = await requests.download(
        attachment.id,
        'original',
        controller.signal,
      );
      const blob = await downloadBlob(grant, controller.signal);
      if (controller.signal.aborted) return;

      const url = URL.createObjectURL(blob);
      const link = document.createElement('a');
      link.href = url;
      link.download = attachment.filename;
      link.click();
      // The browser consumes the href asynchronously after the click.
      setTimeout(() => URL.revokeObjectURL(url), DOWNLOAD_URL_RELEASE_DELAY_MS);
    } catch (reason) {
      error = reason instanceof Error ? reason.message : '下载失败';
    } finally {
      busy = false;
    }
  }

  async function loaded() {
    await tick();
    onLayout();
  }

  onDestroy(() => downloadController?.abort());

  $effect(() => {
    const element = root;
    if (!element) return;
    let intersecting = false;
    const update = () => {
      visible = intersecting && document.visibilityState === 'visible';
    };
    const observer = new IntersectionObserver(([entry]) => {
      intersecting = entry.isIntersecting;
      update();
    });
    observer.observe(element);
    document.addEventListener('visibilitychange', update);
    return () => {
      observer.disconnect();
      document.removeEventListener('visibilitychange', update);
    };
  });

  $effect(() => {
    const id = attachment.id;
    if (!visible || !image) return;
    const controller = new AbortController();
    let current = '';
    let generation = 0;

    async function refresh() {
      const version = ++generation;

      try {
        const grant = await requests.download(
          id,
          'thumbnail',
          controller.signal,
        );
        const blob = await downloadBlob(grant, controller.signal);
        if (controller.signal.aborted || generation !== version) return;

        if (current) URL.revokeObjectURL(current);
        current = URL.createObjectURL(blob);
        preview = current;
        error = '';
      } catch (reason) {
        if (!controller.signal.aborted) {
          error = reason instanceof Error ? reason.message : '预览加载失败';
        }
      }
    }

    void refresh();
    // Renewal is scoped to a visible component; hiding/removal cancels both the
    // authorization request and byte transfer, and releases the local blob URL.
    const timer = setInterval(() => {
      void refresh();
    }, PREVIEW_REFRESH_INTERVAL_MS);
    return () => {
      controller.abort();
      clearInterval(timer);
      if (current) URL.revokeObjectURL(current);
      preview = '';
    };
  });
</script>

<div class="attachment" bind:this={root}>
  {#if image}
    <div
      class="attachment-preview"
      style:aspect-ratio={`${width} / ${height}`}
      style:max-width={`${width}px`}
    >
      {#if preview}
        <img
          src={preview}
          alt={attachment.filename}
          {width}
          {height}
          onload={loaded}
        />
      {:else}
        <span>正在加载预览…</span>
      {/if}
    </div>
  {/if}
  <Button
    type="button"
    variant="ghost"
    class="attachment-download h-auto whitespace-normal"
    disabled={busy}
    onclick={download}
  >
    {busy ? '正在下载…' : attachment.filename}
    <small>{(attachment.size_bytes / 1024).toFixed(1)} KiB</small>
  </Button>
  {#if error}
    <p class="form-error" role="alert">{error}</p>
  {/if}
</div>
