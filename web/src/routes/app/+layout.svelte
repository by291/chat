<script lang="ts">
  import type { Snippet } from 'svelte';
  import { goto } from '$app/navigation';
  import { page } from '$app/state';

  import { auth } from '#lib/features/auth/session.svelte.ts';
  import WorkspaceShell from '#lib/features/workspace/WorkspaceShell.svelte';

  let { children }: { children: Snippet } = $props();

  $effect(() => {
    if (!auth.pending && !auth.user) {
      const pendingJoin =
        page.url.pathname === '/app/groups/join' ||
        /^\/app\/chats\/[0-9a-f-]{36}$/.test(page.url.pathname)
          ? `?next=${encodeURIComponent(page.url.pathname + page.url.search)}`
          : '';
      void goto(`/login${pendingJoin}`);
    }
  });
</script>

<WorkspaceShell {children} />
