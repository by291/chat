<script lang="ts">
  import { onMount, onDestroy } from 'svelte';

  import { Button } from '#lib/components/ui/button/index.js';

  import AuthForm from '../AuthForm.svelte';
  import { createAuthApi } from '../api';
  import { auth } from '../session.svelte.ts';
  import { EmailVerificationState } from './email-verification.svelte.ts';

  const requests = createAuthApi();
  const verification = new EmailVerificationState(
    requests.verifyEmail,
    () => auth.user?.id,
  );
  const userId = $derived(auth.user?.id);
  let previousUserId: string | undefined;
  const unsubscribe = auth.onReset(() => verification.invalidate());

  onMount(() => {
    const read = () => verification.initialize(window.location.hash);
    read();
    window.addEventListener('hashchange', read);
    void auth.restore();
    return () => {
      window.removeEventListener('hashchange', read);
      verification.dispose();
    };
  });

  onDestroy(unsubscribe);

  $effect(() => {
    if (userId !== previousUserId) {
      previousUserId = userId;
      verification.invalidate();
    }
  });

  $effect(() => {
    if (verification.done) {
      // Completion removes the secret from history without navigating or persisting it.
      history.replaceState(
        null,
        '',
        window.location.pathname + window.location.search,
      );
    }
  });
</script>

<svelte:head>
  <title>验证邮箱 · Chat</title>
</svelte:head>
{#if verification.done}
  <main class="auth-main">
    <section class="auth-card">
      <h1>邮箱已验证</h1>
      <p>现在可以通过邮件找回密码。</p>
      <a href="/app/settings">返回个人设置</a>
    </section>
  </main>
{:else if auth.pending}
  <main class="auth-main"><p>正在确认账号…</p></main>
{:else if !auth.user}
  <AuthForm mode="login" onAuthenticated={() => {}} />
{:else}
  <main class="auth-main">
    <section class="auth-card">
      <h1>验证恢复邮箱</h1>
      <p>当前账号：@{auth.user.username}。请使用发起邮箱绑定的账号确认。</p>
      {#if verification.token}
        <Button
          type="button"
          disabled={verification.pending}
          onclick={() => verification.confirm()}
        >
          {verification.pending ? '正在验证…' : '确认验证邮箱'}
        </Button>
      {:else}
        <p class="form-error" role="alert">链接无效或已失效，请重新申请。</p>
      {/if}
      {#if verification.error}
        <p class="form-error" role="alert">
          {verification.error}
        </p>
      {/if}
      <Button type="button" variant="outline" onclick={() => auth.logout()}>
        换个账号登录
      </Button>
      <a href="/app/settings">返回个人设置</a>
    </section>
  </main>
{/if}
