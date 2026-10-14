<script lang="ts">
  import { onMount } from 'svelte';

  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Label } from '#lib/components/ui/label/index.js';
  import { createAuthApi } from './api';
  import { auth } from './session.svelte';
  import { PasswordResetState } from './password-recovery.svelte';

  const reset = new PasswordResetState(createAuthApi().resetPassword);
  let password = $state('');
  let confirmation = $state('');

  onMount(() => {
    const read = () => {
      reset.initialize(window.location.hash);
      password = '';
      confirmation = '';
    };
    read();
    window.addEventListener('hashchange', read);
    return () => {
      window.removeEventListener('hashchange', read);
      reset.dispose();
      password = '';
      confirmation = '';
    };
  });

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    await reset.reset(password, confirmation);
    if (reset.done) {
      password = '';
      confirmation = '';
      history.replaceState(
        null,
        '',
        window.location.pathname + window.location.search,
      );
      // The token may belong to a different account from the current cookie.
      // Revalidation clears a revoked owner but preserves another valid identity.
      if (auth.user) void auth.restore();
    }
  }
</script>

<svelte:head><title>重置密码 · Chat</title></svelte:head>
<main class="auth-main">
  <section class="auth-card">
    {#if reset.done}
      <h1>密码已重置</h1>
      <p>所有设备已退出登录，请使用新密码重新登录。</p>
      <a href="/login">使用新密码登录</a>
    {:else}
      <h1>设置新密码</h1>
      <p>重置成功后，所有设备都会退出登录。</p>
      {#if reset.token}
        <form onsubmit={submit}>
          <Label class="form-label" for="reset-password">新密码</Label>
          <Input
            id="reset-password"
            name="new_password"
            type="password"
            autocomplete="new-password"
            bind:value={password}
            required
          />
          <Label class="form-label" for="reset-confirmation">确认新密码</Label>
          <Input
            id="reset-confirmation"
            name="confirmation"
            type="password"
            autocomplete="new-password"
            bind:value={confirmation}
            required
          />
          <Button class="auth-submit" type="submit" disabled={reset.pending}>
            {reset.pending ? '正在重置…' : '重置密码'}
          </Button>
        </form>
      {/if}
      {#if reset.error}<p class="form-error" role="alert">{reset.error}</p>{/if}
      <p>提交后若连接中断，请先尝试用新密码登录；仍无法登录时再申请邮件。</p>
      <a href="/login">使用新密码登录</a>
      <p><a href="/forgot-password">重新申请找回邮件</a></p>
    {/if}
  </section>
</main>
