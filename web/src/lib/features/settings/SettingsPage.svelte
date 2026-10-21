<script lang="ts">
  import { tick } from 'svelte';
  import { ArrowLeft } from '@lucide/svelte';
  import { goto } from '$app/navigation';
  import { page } from '$app/state';
  import { Button } from '#lib/components/ui/button/index.js';
  import PaneHeader from '#lib/components/PaneHeader.svelte';

  import AppearanceSettings from './appearance/AppearanceSettings.svelte';
  import SettingsNavigation from '#lib/features/settings/SettingsNavigation.svelte';
  import WorkspaceNavigation from '#lib/features/workspace/WorkspaceNavigation.svelte';
  import SessionsPanel from '#lib/features/settings/sessions/SessionsPanel.svelte';
  import PasswordForm from '#lib/features/settings/security/PasswordForm.svelte';
  import EmailForm from '#lib/features/settings/security/EmailForm.svelte';
  import InstallHelp from '#lib/features/pwa/InstallHelp.svelte';
  import BackgroundPushSettings from '#lib/features/notifications/BackgroundPushSettings.svelte';
  import { auth } from '#lib/features/auth/session.svelte.ts';

  const sections: Record<string, { title: string; description: string }> = {
    'settings-security': {
      title: '账号与安全',
      description: '管理密码、恢复邮箱和登录设备。',
    },
    'settings-preferences': {
      title: '通知与外观',
      description: '选择适合你的提醒方式和界面外观。',
    },
    'settings-application': {
      title: '应用',
      description: '在你常用的设备上使用 Chat。',
    },
  };
  const active = $derived(
    Object.hasOwn(sections, page.url.hash.slice(1))
      ? page.url.hash.slice(1)
      : 'settings-security',
  );
  const detail = $derived(Object.hasOwn(sections, page.url.hash.slice(1)));
  let content: HTMLElement;

  $effect(() => {
    void active;
    if (content) content.scrollTop = 0;
  });
</script>

<svelte:head>
  <title>个人设置 · Chat</title>
</svelte:head>
<main class="settings-page workspace-columns" class:settings-detail={detail}>
  <aside class="settings-sidebar" aria-label="设置导航">
    <WorkspaceNavigation />
    <PaneHeader title="设置" level={1} />
    <SettingsNavigation {active} />
  </aside>
  <div class="settings-main">
    <PaneHeader title={sections[active].title}>
      {#snippet leading()}
        <Button
          variant="ghost"
          size="icon"
          class="mobile-back"
          aria-label="返回设置列表"
          onclick={async () => {
            const previous = active;
            await goto('/app/settings', { reset: false });
            await tick();
            document
              .querySelector<HTMLAnchorElement>(
                `.settings-navigation a[href="#${previous}"]`,
              )
              ?.focus();
          }}
        >
          <ArrowLeft class="size-5" />
        </Button>
      {/snippet}
    </PaneHeader>
    <div class="settings-content" bind:this={content}>
      <section
        class="settings-group"
        hidden={active !== 'settings-security'}
        aria-labelledby="settings-security"
      >
        <header class="settings-group-heading">
          <h2 id="settings-security" tabindex="-1">账号与安全</h2>
        </header>
        {#if auth.user}
          <PasswordForm />
          <EmailForm />
        {/if}
        <SessionsPanel />
      </section>
      <section
        class="settings-group"
        hidden={active !== 'settings-preferences'}
        aria-labelledby="settings-preferences"
      >
        <header class="settings-group-heading">
          <h2 id="settings-preferences" tabindex="-1">通知与外观</h2>
        </header>
        <BackgroundPushSettings />
        <AppearanceSettings />
      </section>
      <section
        class="settings-group"
        hidden={active !== 'settings-application'}
        aria-labelledby="settings-application"
      >
        <header class="settings-group-heading">
          <h2 id="settings-application" tabindex="-1">应用</h2>
        </header>
        <InstallHelp />
      </section>
    </div>
  </div>
</main>
