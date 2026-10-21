<script lang="ts">
  import { LogOut } from '@lucide/svelte';

  import { Button } from '#lib/components/ui/button/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';
  import { auth } from '#lib/features/auth/session.svelte.ts';

  import ProfileForm from './ProfileForm.svelte';
  import AvatarEditor from './AvatarEditor.svelte';

  let {
    open = $bindable(false),
    onCloseAutoFocus,
    onLogout,
    signingOut,
    logoutError,
  }: {
    open: boolean;
    onCloseAutoFocus: (event: Event) => void;
    onLogout: () => Promise<void>;
    signingOut: boolean;
    logoutError: string;
  } = $props();
</script>

<Dialog.Root bind:open>
  <Dialog.Content
    class="own-profile-dialog"
    closeLabel="关闭"
    {onCloseAutoFocus}
  >
    <Dialog.Header>
      <Dialog.Title>个人资料</Dialog.Title>
      <Dialog.Description>
        更新头像和昵称，让朋友更容易认出你。
      </Dialog.Description>
    </Dialog.Header>
    {#if auth.user}
      <div class="own-profile-editor">
        <AvatarEditor userId={auth.user.id} name={auth.user.display_name} />
        <ProfileForm
          user={auth.user}
          onSaved={(user) => {
            // The mounted authenticated identity owns this mutation result.
            if (auth.user?.id !== user.id) return;
            auth.user = user;
          }}
        />
      </div>
    {/if}
    <footer class="own-profile-footer">
      <Button
        variant="ghost"
        class="profile-logout"
        disabled={signingOut}
        onclick={onLogout}
      >
        <LogOut size={16} />{signingOut ? '正在退出…' : '退出登录'}
      </Button>
      <span class="muted">仅退出当前设备</span>
      <Button variant="secondary" onclick={() => (open = false)}>完成</Button>
    </footer>
    {#if logoutError}
      <p class="form-error" role="alert">{logoutError}</p>
    {/if}
  </Dialog.Content>
</Dialog.Root>
