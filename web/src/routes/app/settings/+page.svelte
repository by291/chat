<script lang="ts">
  import { ArrowLeft } from '@lucide/svelte';

  import * as Card from '#lib/components/ui/card/index.js';
  import * as NativeSelect from '#lib/components/ui/native-select/index.js';
  import { Label } from '#lib/components/ui/label/index.js';

  import SettingsNavigation from '#lib/features/settings/SettingsNavigation.svelte';
  import ProfileForm from '#lib/features/settings/ProfileForm.svelte';
  import AvatarEditor from '#lib/features/settings/AvatarEditor.svelte';
  import SessionsPanel from '#lib/features/settings/SessionsPanel.svelte';
  import PasswordForm from '#lib/features/settings/PasswordForm.svelte';
  import EmailForm from '#lib/features/settings/EmailForm.svelte';
  import InstallHelp from '#lib/features/pwa/InstallHelp.svelte';
  import BackgroundPushSettings from '#lib/features/notifications/BackgroundPushSettings.svelte';
  import { theme, type Theme } from '#lib/features/settings/theme.svelte.ts';
  import { auth } from '#lib/features/auth/session.svelte.ts';
</script>

<svelte:head>
  <title>个人设置 · Chat</title>
</svelte:head>
<main class="settings-page">
  <div class="settings-container">
    <a class="back-link" href="/app"><ArrowLeft size={16} />返回聊天</a>
    <header class="page-heading">
      <span class="eyebrow">MAKE IT YOURS</span>
      <h1>个人设置</h1>
      <p class="muted">管理个人资料、账号安全和使用偏好。</p>
    </header>
  </div>
  <SettingsNavigation />
  <div class="settings-content settings-container">
    {#if auth.user}
      <section class="settings-group" aria-labelledby="settings-profile">
        <header class="settings-group-heading">
          <h2 id="settings-profile" tabindex="-1">个人资料</h2>
        </header>
        <AvatarEditor userId={auth.user.id} name={auth.user.display_name} />
        <ProfileForm
          user={auth.user}
          onSaved={(user) => {
            auth.user = user;
          }}
        />
      </section>
    {/if}
    <section class="settings-group" aria-labelledby="settings-security">
      <header class="settings-group-heading">
        <h2 id="settings-security" tabindex="-1">账号与安全</h2>
      </header>
      {#if auth.user}
        <PasswordForm />
        <EmailForm />
      {/if}
      <SessionsPanel />
    </section>
    <section class="settings-group" aria-labelledby="settings-preferences">
      <header class="settings-group-heading">
        <h2 id="settings-preferences" tabindex="-1">通知与外观</h2>
      </header>
      <BackgroundPushSettings />
      <Card.Root
        class="settings-card gap-0 p-6"
        role="region"
        aria-label="界面外观"
      >
        <div class="section-heading">
          <h3>界面外观</h3>
        </div>
        <Label class="form-label" for="theme">外观</Label>
        <NativeSelect.Root
          class="w-full"
          id="theme"
          value={theme.value}
          onchange={(e) => theme.set(e.currentTarget.value as Theme)}
        >
          <NativeSelect.Option value="system">跟随系统</NativeSelect.Option>
          <NativeSelect.Option value="light">浅色</NativeSelect.Option>
          <NativeSelect.Option value="dark">深色</NativeSelect.Option>
        </NativeSelect.Root>
      </Card.Root>
    </section>
    <section class="settings-group" aria-labelledby="settings-application">
      <header class="settings-group-heading">
        <h2 id="settings-application" tabindex="-1">应用</h2>
      </header>
      <InstallHelp />
    </section>
  </div>
</main>
