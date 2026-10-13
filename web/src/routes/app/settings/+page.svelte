<script lang="ts">
  import * as Card from '#lib/components/ui/card/index.js';
  import * as NativeSelect from '#lib/components/ui/native-select/index.js';
  import { Label } from '#lib/components/ui/label/index.js';

  import ProfileForm from '#lib/features/settings/ProfileForm.svelte';
  import AvatarEditor from '#lib/features/settings/AvatarEditor.svelte';
  import SessionsPanel from '#lib/features/settings/SessionsPanel.svelte';
  import { ArrowLeft } from '@lucide/svelte';
  import { theme, type Theme } from '#lib/features/settings/theme.svelte.ts';
  import { auth } from '#lib/features/auth/session.svelte.ts';
</script>

<svelte:head><title>个人设置 · Chat</title></svelte:head>
<main class="settings-page">
  <a class="back-link" href="/app"><ArrowLeft size={16} />返回聊天</a>
  <header class="page-heading">
    <span class="eyebrow">MAKE IT YOURS</span>
    <h1>个人设置</h1>
    <p class="muted">你的资料、设备和聊天偏好。</p>
  </header>
  {#if auth.user}
    <AvatarEditor userId={auth.user.id} name={auth.user.display_name} />
    <ProfileForm
      user={auth.user}
      onSaved={(user) => {
        auth.user = user;
      }}
    />
  {/if}
  <Card.Root
    class="settings-card gap-0 p-6"
    role="region"
    aria-label="聊天偏好"
  >
    <div class="section-heading">
      <h2>聊天偏好</h2>
      <p class="muted">选择看起来舒服的界面。</p>
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
  <SessionsPanel />
</main>
