<script lang="ts">
  import { onDestroy } from 'svelte';

  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Label } from '#lib/components/ui/label/index.js';

  import { createAuthApi } from './api';

  const requests = createAuthApi();
  let email = $state('');
  let pending = $state(false);
  let acknowledged = $state(false);
  let error = $state('');
  let generation = 0;
  let request: AbortController | undefined;

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (pending) return;
    const currentGeneration = ++generation;
    request = new AbortController();
    pending = true;
    error = '';
    acknowledged = false;

    try {
      await requests.requestRecovery(email, request.signal);
      if (currentGeneration === generation) {
        acknowledged = true;
        email = '';
      }
    } catch (reason) {
      if (currentGeneration === generation) {
        error =
          reason instanceof Error ? reason.message : '申请失败，请稍后重试';
      }
    } finally {
      if (currentGeneration === generation) {
        pending = false;
        request = undefined;
      }
    }
  }

  onDestroy(() => {
    generation++;
    request?.abort();
    email = '';
  });
</script>

<svelte:head>
  <title>找回密码 · Chat</title>
</svelte:head>
<main class="auth-main">
  <section class="auth-card">
    <h1>找回密码</h1>
    <p>使用个人设置中已验证的邮箱申请找回邮件。</p>
    <form onsubmit={submit}>
      <Label class="form-label" for="recovery-email">已验证邮箱</Label>
      <Input
        id="recovery-email"
        name="email"
        type="email"
        autocomplete="email"
        bind:value={email}
        required
        maxlength={320}
      />
      <Button class="auth-submit" type="submit" disabled={pending}>
        {pending ? '正在申请…' : '申请找回邮件'}
      </Button>
    </form>
    {#if acknowledged}
      <p role="status">
        如果此邮箱已验证，找回邮件会发送到该邮箱。链接在 15
        分钟后失效；请检查垃圾邮件。
      </p>
    {/if}
    {#if error}
      <p class="form-error" role="alert">{error}</p>
    {/if}
    <p>没有绑定邮箱或无法收信时，请联系管理员协助找回。</p>
    <a href="/login">返回登录</a>
  </section>
</main>
