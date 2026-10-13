<script lang="ts">
  import { onMount, type Snippet } from 'svelte';
  import { theme } from '#lib/features/settings/theme.svelte.ts';
  import '../app.css';

  let {
    children,
  }: {
    children: Snippet;
  } = $props();

  onMount(() => theme.init());
</script>

{@render children()}
