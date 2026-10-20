<script lang="ts">
  import { goto } from '$app/navigation';
  import { LogOut, UserRound } from '@lucide/svelte';

  import { Button } from '#lib/components/ui/button/index.js';
  import * as DropdownMenu from '#lib/components/ui/dropdown-menu/index.js';
  import { auth } from '#lib/features/auth/session.svelte.ts';
  import { useBackgroundPush } from '#lib/features/notifications/context.svelte.ts';

  import ProfileDialog from './ProfileDialog.svelte';

  let { mobile = false }: { mobile?: boolean } = $props();
  const push = useBackgroundPush();
  let profileOpen = $state(false);
  let trigger = $state<HTMLButtonElement | null>(null);
  let signingOut = $state(false);
  let error = $state('');

  async function logout() {
    if (signingOut) return;
    signingOut = true;
    error = '';
    try {
      await push.owner?.disable();
      await auth.logout();
      await goto('/login');
    } catch (reason) {
      error = reason instanceof Error ? reason.message : '退出失败';
    } finally {
      signingOut = false;
    }
  }
</script>

<div class:mobile-account={mobile} class:rail-account={!mobile}>
  <DropdownMenu.Root>
    <DropdownMenu.Trigger aria-label="我的账号">
      {#snippet child({ props })}
        <Button
          bind:ref={trigger}
          variant="secondary"
          size="icon"
          class="workspace-account"
          {...props}
        >
          {Array.from(auth.user?.display_name ?? '')[0] ?? '我'}
        </Button>
      {/snippet}
    </DropdownMenu.Trigger>
    <DropdownMenu.Content
      side={mobile ? 'bottom' : 'right'}
      align="end"
      class="w-56"
      onCloseAutoFocus={(event) => {
        if (profileOpen) event.preventDefault();
      }}
    >
      <DropdownMenu.Label>
        <strong>{auth.user?.display_name}</strong>
        <p class="muted">@{auth.user?.username}</p>
      </DropdownMenu.Label>
      <DropdownMenu.Separator />
      <DropdownMenu.Item onSelect={() => (profileOpen = true)}>
        <UserRound size={16} />个人资料
      </DropdownMenu.Item>
      <DropdownMenu.Item disabled={signingOut} onSelect={logout}>
        <LogOut size={16} />退出登录
      </DropdownMenu.Item>
    </DropdownMenu.Content>
  </DropdownMenu.Root>
</div>
<ProfileDialog
  bind:open={profileOpen}
  onCloseAutoFocus={(event) => {
    event.preventDefault();
    trigger?.focus();
  }}
/>
{#if error}<p class="toast error" role="alert">{error}</p>{/if}
