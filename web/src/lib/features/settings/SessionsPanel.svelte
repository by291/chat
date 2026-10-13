<script lang="ts">
  import * as Card from '#lib/components/ui/card/index.js';
  import { Button } from '#lib/components/ui/button/index.js';

  import { onMount } from 'svelte';
  import { Monitor, ShieldCheck } from '@lucide/svelte';
  import { settingsApi } from './api';
  import type { components } from '../../api-client';

  let sessions = $state<components['schemas']['SessionView'][]>([]);
  let pending = $state<string | null>(null);
  let error = $state('');

  async function load() {
    try {
      sessions = await settingsApi.sessions();
    } catch (reason) {
      error = reason instanceof Error ? reason.message : '加载失败';
    }
  }

  async function revoke(id: string) {
    if (pending) {
      return;
    }
    pending = id;
    error = '';

    try {
      await settingsApi.revokeSession(id);
      await load();
    } catch (reason) {
      error = reason instanceof Error ? reason.message : '撤销失败';
    } finally {
      pending = null;
    }
  }

  onMount(() => {
    void load();
  });
</script>

<Card.Root class="settings-card gap-0 p-6" role="region" aria-label="登录设备">
  <div class="section-heading">
    <span class="eyebrow">SECURITY</span>
    <h2>登录设备</h2>
    <p class="muted">撤销不再使用的设备，让账号保持安全。</p>
  </div>
  <div class="sessions-list">
    {#each sessions as session (session.id)}
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
          disabled={pending !== null}
          onclick={() => revoke(session.id)}
        >
          {pending === session.id
            ? '撤销中…'
            : session.is_current
              ? '撤销此设备'
              : '撤销'}
        </Button>
      </div>
    {/each}
  </div>
  {#if error}
    <p class="form-error" role="alert">{error}</p>
  {/if}
</Card.Root>
