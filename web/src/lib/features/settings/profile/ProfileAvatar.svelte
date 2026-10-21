<script lang="ts">
  import * as Avatar from '#lib/components/ui/avatar/index.js';
  import { avatarTone } from '#lib/utils.js';

  import { downloadBlob } from '#lib/features/media/api.ts';

  import { settingsApi } from '../api';

  const AVATAR_REFRESH_INTERVAL_MS = 60_000;

  let {
    userId,
    name,
    revision = 0,
  }: { userId: string; name: string; revision?: number } = $props();

  let url = $state('');

  $effect(() => {
    const id = userId;
    void revision;
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
    }, AVATAR_REFRESH_INTERVAL_MS);
    return () => {
      controller.abort();
      clearInterval(timer);
      if (current) URL.revokeObjectURL(current);
      url = '';
    };
  });
</script>

<!-- Bits UI retains loaded status on Root; reset it when the image disappears. -->
{#key !!url}
  <Avatar.Root data-tone={avatarTone(userId)}>
    {#if url}
      <Avatar.Image src={url} alt={`${name}的头像`} />
    {/if}
    <Avatar.Fallback>{Array.from(name)[0] ?? '…'}</Avatar.Fallback>
  </Avatar.Root>
{/key}
