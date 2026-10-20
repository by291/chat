<script lang="ts">
  import type { Snippet } from 'svelte';

  let {
    title,
    level = 2,
    leading,
    subtitle,
    actions,
  }: {
    title: string;
    level?: 1 | 2;
    leading?: Snippet;
    subtitle?: Snippet;
    actions?: Snippet;
  } = $props();
</script>

<header class="pane-heading">
  {#if leading}<div class="pane-leading">{@render leading()}</div>{/if}
  <div class="pane-title">
    {#if level === 1}<h1>{title}</h1>{:else}<h2 tabindex="-1">{title}</h2>{/if}
    {#if subtitle}<div class="pane-subtitle">{@render subtitle()}</div>{/if}
  </div>
  {#if actions}<div class="pane-actions">{@render actions()}</div>{/if}
</header>
