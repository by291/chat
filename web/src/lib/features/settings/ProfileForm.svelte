<script lang="ts">
  import { onDestroy, untrack } from 'svelte';

  import * as Card from '#lib/components/ui/card/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Label } from '#lib/components/ui/label/index.js';
  import type { components } from '#lib/api-client';

  import { settingsApi } from './api';

  let {
    user,
    onSaved,
    onCancel,
  }: {
    user: components['schemas']['UserView'];
    onSaved: (user: components['schemas']['UserView']) => void;
    onCancel?: () => void;
  } = $props();

  // Initialize the editable draft once; same-user profile revalidation must not
  // overwrite text the user has not submitted yet.
  let displayName = $state(untrack(() => user.display_name));
  let pending = $state(false);
  let error = $state('');
  let saved = $state(false);
  const lifetime = new AbortController();

  onDestroy(() => lifetime.abort());

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (pending) {
      return;
    }
    pending = true;
    error = '';
    saved = false;

    try {
      const updated = await settingsApi.updateProfile(
        displayName.trim() || user.username,
        lifetime.signal,
      );
      // Closing and reopening the dialog starts a new draft; late saves cannot replace it.
      if (lifetime.signal.aborted) return;
      onSaved(updated);
      saved = true;
    } catch (reason) {
      if (!lifetime.signal.aborted)
        error = reason instanceof Error ? reason.message : '保存失败';
    } finally {
      pending = false;
    }
  }
</script>

<Card.Root class="settings-card gap-0 p-6" role="region" aria-label="基本资料">
  <div class="section-heading">
    <h3>基本资料</h3>
  </div>
  <form onsubmit={submit}>
    <Label class="form-label" for="profile-username">用户名</Label>
    <Input id="profile-username" value={`@${user.username}`} readonly />
    <p class="field-help">用户名是你的唯一标识。</p>
    <Label class="form-label" for="profile-name">昵称</Label>
    <Input id="profile-name" bind:value={displayName} maxlength={100} />
    <p class="field-help">选填，留空时显示用户名。</p>
    <div class="form-actions">
      <Button type="submit" disabled={pending}>
        {pending ? '保存中…' : '保存资料'}
      </Button>
      {#if onCancel}<Button
          variant="secondary"
          disabled={pending}
          onclick={onCancel}
        >
          取消
        </Button>{/if}
      {#if saved}
        <span class="success" role="status">资料已保存</span>
      {/if}
    </div>
    {#if error}
      <p class="form-error" role="alert">{error}</p>
    {/if}
  </form>
</Card.Root>
