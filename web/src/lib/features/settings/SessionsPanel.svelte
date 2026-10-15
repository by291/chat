<script lang="ts">
  import { untrack } from 'svelte';
  import { useQueryClient } from '@tanstack/svelte-query';
  import { Monitor, ShieldCheck } from '@lucide/svelte';

  import * as Card from '#lib/components/ui/card/index.js';
  import { Button } from '#lib/components/ui/button/index.js';

  import { auth } from '#lib/features/auth/session.svelte.ts';

  import { SettingsSessions } from './sessions.svelte';

  const queryClient = useQueryClient();
  let owner = $state<SettingsSessions | null>(null);

  const authenticatedId = $derived(auth.user?.id);

  $effect(() => {
    const userId = authenticatedId;
    if (!userId) return;

    const sessions = new SettingsSessions({ queryClient, userId });
    owner = sessions;
    const removeReset = auth.onReset(() => sessions.stop());
    // Identity owns this lifetime; loading/cache feedback must not subscribe the
    // effect to records and restart it during same-user profile revalidation.
    untrack(() => void sessions.load());

    return () => {
      removeReset();
      sessions.stop();
      owner = null;
    };
  });
</script>

<Card.Root class="settings-card gap-0 p-6" role="region" aria-label="登录设备">
  <div class="section-heading">
    <span class="eyebrow">SECURITY</span>
    <h2>登录设备</h2>
    <p class="muted">撤销不再使用的设备，让账号保持安全。</p>
  </div>
  <div class="sessions-list">
    {#each owner?.sessions ?? [] as session (session.id)}
      <div class="session-item">
        <div class="device-icon"><Monitor size={22} /></div>
        <div class="session-description">
          <strong>{session.device_label}</strong>
          <span>
            登录于 {new Date(session.created_at).toLocaleString('zh-CN')}
          </span>
          {#if session.is_current}
            <span class="current-device">
              <ShieldCheck size={13} />当前设备
            </span>
          {/if}
        </div>
        <Button
          variant="secondary"
          disabled={owner?.pending !== null}
          onclick={() => owner?.revoke(session.id)}
        >
          {owner?.pending === session.id
            ? '撤销中…'
            : session.is_current
              ? '撤销此设备'
              : '撤销'}
        </Button>
      </div>
    {/each}
  </div>
  {#if owner?.error}
    <p class="form-error" role="alert">{owner.error}</p>
  {/if}
</Card.Root>
