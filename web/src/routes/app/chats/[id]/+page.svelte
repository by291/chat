<script lang="ts">
  import { page } from '$app/state';

  import ChatShell from '#lib/features/chat/ChatShell.svelte';
</script>

<ChatShell id={page.params.id} />
