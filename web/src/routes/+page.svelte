<script lang="ts">
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';

  onMount(() => {
    void goto('/app');
  });
</script>

<svelte:head>
  <title>Chat · 团队聊天</title>
</svelte:head>
<main class="loading-page">
  <span class="brand-mark">C</span>
  <p>正在打开你的聊天空间…</p>
</main>
