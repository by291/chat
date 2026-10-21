<script lang="ts">
  import { onDestroy } from 'svelte';

  import * as Card from '#lib/components/ui/card/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Label } from '#lib/components/ui/label/index.js';

  import { auth } from '#lib/features/auth/session.svelte.ts';

  let currentPassword = $state('');
  let newPassword = $state('');
  let confirmation = $state('');
  let pending = $state(false);
  let error = $state('');
  let requestGeneration = 0;
  let formUserId: string | undefined;

  const authenticatedId = $derived(auth.user?.id);

  function clearPasswords() {
    currentPassword = '';
    newPassword = '';
    confirmation = '';
  }

  function resetForm() {
    requestGeneration++;
    clearPasswords();

    pending = false;
    error = '';
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (pending) return;
    error = '';

    if (newPassword !== confirmation) {
      error = '两次输入的新密码不一致';
      return;
    }
    const passwordLength = Array.from(newPassword).length;
    if (passwordLength < 6 || passwordLength > 128) {
      error = '密码需要 6–128 个字符';
      return;
    }

    const generation = requestGeneration;
    pending = true;
    try {
      await auth.changePassword({
        current_password: currentPassword,
        new_password: newPassword,
      });
      if (generation === requestGeneration) clearPasswords();
    } catch (reason) {
      if (generation === requestGeneration) {
        error =
          reason instanceof Error ? reason.message : '修改密码失败，请重试';
      }
    } finally {
      if (generation === requestGeneration) pending = false;
    }
  }

  const unsubscribe = auth.onReset(resetForm);

  $effect(() => {
    if (authenticatedId !== formUserId) {
      formUserId = authenticatedId;
      resetForm();
    }
  });

  onDestroy(() => {
    unsubscribe();
    // A mutation may still commit after navigation. Clear secrets immediately,
    // fence detached form feedback, and let the session owner handle revocation.
    resetForm();
  });
</script>

<Card.Root class="settings-card gap-0 p-6" role="region" aria-label="账号安全">
  <div class="section-heading">
    <h3>修改密码</h3>
    <p class="muted">
      修改成功后，所有设备都会退出登录，需要使用新密码重新登录。
    </p>
  </div>
  <form onsubmit={submit}>
    <Label class="form-label" for="current-password">当前密码</Label>
    <Input
      id="current-password"
      type="password"
      autocomplete="current-password"
      bind:value={currentPassword}
      required
    />
    <Label class="form-label" for="new-password">新密码</Label>
    <Input
      id="new-password"
      type="password"
      autocomplete="new-password"
      bind:value={newPassword}
      required
    />
    <p class="field-help">6–128 个字符，可包含中文和表情。</p>
    <Label class="form-label" for="confirm-new-password">确认新密码</Label>
    <Input
      id="confirm-new-password"
      type="password"
      autocomplete="new-password"
      bind:value={confirmation}
      required
    />
    <div class="form-actions">
      <Button type="submit" disabled={pending}>
        {pending ? '修改中…' : '修改密码'}
      </Button>
    </div>
    {#if error}
      <p class="form-error" role="alert">{error}</p>
    {/if}
  </form>
</Card.Root>
