<script lang="ts">
  import { onDestroy } from 'svelte';
  import { createQuery, useQueryClient } from '@tanstack/svelte-query';

  import * as Card from '#lib/components/ui/card/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Label } from '#lib/components/ui/label/index.js';
  import { createAuthApi } from '#lib/features/auth/api.ts';
  import { auth } from '#lib/features/auth/session.svelte.ts';

  const requests = createAuthApi();
  const queryClient = useQueryClient();
  const userId = $derived(auth.user?.id);
  const emailStatus = createQuery(() => ({
    queryKey: ['account-email', userId],
    queryFn: ({ signal }) => requests.emailStatus(signal),
    enabled: !!userId,
  }));

  let email = $state('');
  let currentPassword = $state('');
  let pending = $state(false);
  let requested = $state(false);
  let error = $state('');
  let generation = 0;
  let formUserId: string | undefined;

  function resetForm() {
    generation++;
    email = '';
    currentPassword = '';
    pending = false;
    requested = false;
    error = '';
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (pending || !emailStatus.data?.enabled) return;
    const requestGeneration = generation;
    const owner = userId;
    pending = true;
    requested = false;
    error = '';

    try {
      await requests.enrollEmail({ email, current_password: currentPassword });
      if (requestGeneration === generation && owner === userId) {
        currentPassword = '';
        requested = true;
        await queryClient.invalidateQueries({
          queryKey: ['account-email', owner],
        });
      }
    } catch (reason) {
      if (requestGeneration === generation && owner === userId) {
        error = reason instanceof Error ? reason.message : '申请失败，请重试';
      }
    } finally {
      if (requestGeneration === generation) pending = false;
    }
  }

  const unsubscribe = auth.onReset(resetForm);
  $effect(() => {
    if (userId !== formUserId) {
      formUserId = userId;
      resetForm();
    }
  });
  onDestroy(() => {
    unsubscribe();
    resetForm();
  });
</script>

<Card.Root class="settings-card gap-0 p-6" role="region" aria-label="恢复邮箱">
  <div class="section-heading">
    <h2>恢复邮箱</h2>
    <p class="muted">验证邮箱后，可以通过邮件找回忘记的密码。</p>
  </div>
  {#if emailStatus.isPending}
    <p class="muted">正在加载邮箱设置…</p>
  {:else if emailStatus.error}
    <p class="form-error" role="alert">{emailStatus.error.message}</p>
  {:else if emailStatus.data}
    {#if emailStatus.data.verified_email}
      <p>
        已验证：
        <strong>{emailStatus.data.verified_email}</strong>
      </p>
    {:else}
      <p class="muted">暂未验证恢复邮箱。</p>
    {/if}
    {#if emailStatus.data.pending_email}
      <p>等待验证：{emailStatus.data.pending_email}</p>
      <p class="field-help">更换邮箱前，原来的已验证邮箱仍然有效。</p>
    {/if}
    {#if emailStatus.data.enabled}
      <form onsubmit={submit}>
        <Label class="form-label" for="recovery-email">恢复邮箱</Label>
        <Input
          id="recovery-email"
          type="email"
          autocomplete="email"
          bind:value={email}
          required
        />
        <Label class="form-label" for="email-current-password">
          验证当前密码
        </Label>
        <Input
          id="email-current-password"
          type="password"
          autocomplete="current-password"
          bind:value={currentPassword}
          required
        />
        <div class="form-actions">
          <Button type="submit" disabled={pending}>
            {pending ? '申请中…' : '发送验证邮件'}
          </Button>
        </div>
        {#if requested}<p class="success" role="status">
            验证邮件已申请，请查收邮箱。
          </p>{/if}
        {#if error}<p class="form-error" role="alert">{error}</p>{/if}
      </form>
    {:else}
      <p class="field-help">邮件服务暂未启用，忘记密码时请联系管理员。</p>
    {/if}
  {/if}
</Card.Root>
