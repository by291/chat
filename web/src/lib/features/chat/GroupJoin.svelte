<script lang="ts">
  import { page } from '$app/state';
  import { goto } from '$app/navigation';

  import { auth } from '../auth/session.svelte';
  import { createGroupApi } from './api';
  import { useChat } from './context.svelte';

  const chat = useChat();
  const requests = createGroupApi();
  const token = $derived(page.url.searchParams.get('invite') ?? '');
  let busy = $state(false);
  let error = $state('');

  async function join() {
    if (!auth.user || !chat.sync?.alive || busy) return;
    const userId = auth.user.id;
    const scope = chat.sync;
    busy = true;
    error = '';
    try {
      const group = await requests.accept(token);
      if (auth.user?.id !== userId || !scope?.alive || chat.sync !== scope)
        return;
      await chat.sync?.refreshSummaries();
      // Replace the token-bearing route after acceptance rather than retaining it in history.
      await goto(`/app/chats/${group.id}`, { replaceState: true });
    } catch (reason) {
      error = reason instanceof Error ? reason.message : '加入失败';
    } finally {
      busy = false;
    }
  }
</script>

<svelte:head><title>加入群聊 · Chat</title></svelte:head>
<section class="group-join-page">
  <h1>加入私人群聊</h1>
  <p class="muted">
    确认加入邀请中的群聊。加入后，只能看到本次加入之后的消息。
  </p>
  <button
    class="button primary"
    disabled={busy || !token || !chat.sync?.alive}
    onclick={join}
  >
    {busy ? '正在加入…' : '确认加入'}
  </button>
  <a class="button subtle" href="/app">返回聊天</a>
  {#if error}<p class="form-error" role="alert">{error}</p>{/if}
</section>
