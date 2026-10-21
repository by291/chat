<script lang="ts">
  import { goto } from '$app/navigation';

  import { Button } from '#lib/components/ui/button/index.js';
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
  <Button
    bind:ref={trigger}
    variant="secondary"
    size="icon"
    class="workspace-account"
    aria-label="我的账号"
    aria-haspopup="dialog"
    aria-expanded={profileOpen}
    onclick={() => (profileOpen = true)}
  >
    {Array.from(auth.user?.display_name ?? '')[0] ?? '我'}
  </Button>
</div>
<ProfileDialog
  bind:open={profileOpen}
  onLogout={logout}
  {signingOut}
  logoutError={error}
  onCloseAutoFocus={(event) => {
    event.preventDefault();
    trigger?.focus();
  }}
/>
