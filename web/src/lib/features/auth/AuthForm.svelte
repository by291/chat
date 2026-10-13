<script lang="ts">
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Label } from '#lib/components/ui/label/index.js';

  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { page } from '$app/state';
  import { MessageCircle, ArrowRight, ShieldCheck } from '@lucide/svelte';
  import { auth } from './session.svelte';

  let {
    mode = 'login',
  }: {
    mode?: 'login' | 'register';
  } = $props();

  let username = $state('');
  let displayName = $state('');
  let password = $state('');
  let invitation = $state('');
  let error = $state('');

  const registering = $derived(mode === 'register');

  function destination() {
    const next = page.url.searchParams.get('next');
    if (!next) return '/app';
    try {
      const target = new URL(next, page.url.origin);
      // Only the new same-origin join workflow may retain a token through login.
      return target.origin === page.url.origin &&
        target.pathname === '/app/groups/join'
        ? target.pathname + target.search
        : '/app';
    } catch {
      // Untrusted malformed redirect values fall back to the ordinary application entry.
      return '/app';
    }
  }

  onMount(() => {
    invitation = page.url.searchParams.get('invite') ?? '';
    void auth.restore().then(() => {
      if (auth.user) {
        void goto(destination());
      }
    });
  });

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (auth.pending) {
      return;
    }
    error = '';

    try {
      if (registering) {
        await auth.register({
          username,
          display_name: displayName,
          password,
          invitation,
        });
      } else {
        await auth.login({
          username,
          password,
          device_label: `${navigator.platform || '浏览器'} · Web`,
        });
      }
      password = '';
      await goto(destination());
    } catch (reason) {
      error = reason instanceof Error ? reason.message : '请稍后重试';
    }
  }
</script>

<svelte:head>
  <title>{registering ? '创建账号' : '登录'} · Chat</title>
</svelte:head>
<div class="auth-page">
  <section class="auth-story">
    <a class="brand" href="/">
      <span class="brand-mark"><MessageCircle size={25} /></span>
      Chat
      <span class="brand-dot">.</span>
    </a>
    <div class="story-copy">
      <span class="eyebrow">让交流自然发生</span>
      <h1>
        每一句话，
        <br />
        都有一个
        <br />
        <span>愿意听的人。</span>
      </h1>
      <p>
        给朋友留一句问候，和团队分享一个想法。
        <br />
        在这里，保持联系是一件简单的事。
      </p>
    </div>
    <div class="story-foot">
      <span class="online-dot"></span>
      小小的团队，也有自己的空间。
    </div>
  </section>
  <main class="auth-main">
    <div class="auth-card">
      <span class="eyebrow">{registering ? '很高兴认识你' : '欢迎回来'}</span>
      <h2>{registering ? '加入你的团队' : '开启一段对话'}</h2>
      <p class="muted">
        {registering
          ? '使用朋友或管理员给你的邀请，创建账号。'
          : '登录后，接着聊上次没说完的话。'}
      </p>
      <form onsubmit={submit}>
        {#if registering}
          <Label class="form-label" for="invitation">邀请码</Label>
          <Input
            id="invitation"
            name="invitation"
            bind:value={invitation}
            autocomplete="off"
            required
            placeholder="粘贴你的邀请码"
          />
        {/if}
        <Label class="form-label" for="username">用户名</Label>
        <div class="input-prefix">
          <span aria-hidden="true">@</span>
          <Input
            id="username"
            name="username"
            bind:value={username}
            autocomplete="username"
            required
            placeholder="你的唯一用户名"
          />
        </div>
        {#if registering}
          <Label class="form-label" for="display-name">昵称</Label>
          <Input
            id="display-name"
            name="display_name"
            bind:value={displayName}
            autocomplete="nickname"
            required
            placeholder="朋友怎么称呼你"
          />
        {/if}
        <Label class="form-label" for="password">密码</Label>
        <Input
          id="password"
          name="password"
          type="password"
          bind:value={password}
          autocomplete={registering ? 'new-password' : 'current-password'}
          required
          placeholder={registering ? '至少 12 个字符' : '输入你的密码'}
        />
        {#if error}
          <p class="form-error" role="alert">{error}</p>
        {/if}
        <Button class="auth-submit" type="submit" disabled={auth.pending}>
          {auth.pending
            ? '请稍候…'
            : registering
              ? '创建账号'
              : '登录'}<ArrowRight size={18} />
        </Button>
      </form>
      <p class="auth-switch">
        {registering ? '已有账号？' : '第一次来到这里？'}
        <a href={registering ? '/login' : '/register'}>
          {registering ? '去登录' : '使用邀请注册'}
        </a>
      </p>
      <div class="auth-note">
        <ShieldCheck size={16} />
        <span>你的登录凭据只由浏览器的安全 Cookie 保存。</span>
      </div>
    </div>
  </main>
</div>
