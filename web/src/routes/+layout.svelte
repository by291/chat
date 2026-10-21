<script lang="ts">
  import { onMount, type Snippet } from 'svelte';
  import { goto } from '$app/navigation';

  import { theme } from '#lib/features/settings/appearance/theme.svelte.ts';
  import { pwa } from '#lib/features/pwa/lifecycle.svelte.ts';

  import { notificationPath } from '../service-worker/push';
  import '../app.css';

  let {
    children,
  }: {
    children: Snippet;
  } = $props();

  onMount(() => {
    theme.init();
    void pwa.start();
    const openNotification = (event: MessageEvent) => {
      if (event.data?.type === 'open-notification') {
        void goto(notificationPath(event.data.path));
      }
    };
    navigator.serviceWorker?.addEventListener('message', openNotification);
    return () => {
      pwa.stop();
      navigator.serviceWorker?.removeEventListener('message', openNotification);
    };
  });
</script>

{@render children()}
