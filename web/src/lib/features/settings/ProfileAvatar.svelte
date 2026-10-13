<script lang="ts">
  import { settingsApi } from './api';
  import { downloadBlob } from '../media/api';

  let {
    userId,
    name,
    revision = 0,
  }: { userId: string; name: string; revision?: number } = $props();
  let url = $state('');

  $effect(() => {
    const id = userId;
    revision;
    const controller = new AbortController();
    let current = '';
    let generation = 0;
    async function refresh() {
      const version = ++generation;
      try {
        const grant = await settingsApi.avatar(id, controller.signal);
        const blob = grant
          ? await downloadBlob(grant, controller.signal)
          : null;
        if (controller.signal.aborted || generation !== version) return;
        if (current) URL.revokeObjectURL(current);
        current = blob ? URL.createObjectURL(blob) : '';
        url = current;
      } catch {
        // Avatars are optional decoration; failed grants retain the initials and
        // retry on the next bounded refresh. No durable mutation is suppressed.
        if (!controller.signal.aborted && generation === version) url = '';
      }
    }
    void refresh();
    const timer = setInterval(() => {
      if (document.visibilityState === 'visible') void refresh();
    }, 60_000);
    return () => {
      controller.abort();
      clearInterval(timer);
      if (current) URL.revokeObjectURL(current);
      url = '';
    };
  });
</script>

<span class="avatar">
  {#if url}<img src={url} alt={`${name}的头像`} />{:else}{Array.from(name)[0] ??
      '…'}{/if}
</span>
