<script lang="ts">
  import { untrack } from 'svelte';

  import * as Card from '#lib/components/ui/card/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Label } from '#lib/components/ui/label/index.js';
  import type { components } from '#lib/api-client';

  import { settingsApi } from './api';

  let {
    user,
    onSaved,
  }: {
    user: components['schemas']['UserView'];
    onSaved: (user: components['schemas']['UserView']) => void;
  } = $props();

  // Initialize the editable draft once; same-user profile revalidation must not
  // overwrite text the user has not submitted yet.
  let displayName = $state(untrack(() => user.display_name));
  let pending = $state(false);
  let error = $state('');
  let saved = $state(false);

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (pending) {
      return;
    }
    pending = true;
    error = '';
    saved = false;

    try {
      onSaved(await settingsApi.updateProfile(displayName));
      saved = true;
    } catch (reason) {
      error = reason instanceof Error ? reason.message : '保存失败';
    } finally {
      pending = false;
    }
  }
</script>

<Card.Root class="settings-card gap-0 p-6" role="region" aria-label="个人资料">
  <div class="section-heading">
    <span class="eyebrow">PROFILE</span>
    <h2>个人资料</h2>
    <p class="muted">让朋友更容易认出你。</p>
  </div>
  <form onsubmit={submit}>
    <Label class="form-label" for="profile-username">用户名</Label>
    <Input id="profile-username" value={`@${user.username}`} readonly />
    <p class="field-help">用户名是你的唯一标识。</p>
    <Label class="form-label" for="profile-name">昵称</Label>
    <Input id="profile-name" bind:value={displayName} required />
    <div class="form-actions">
      <Button type="submit" disabled={pending}>
        {pending ? '保存中…' : '保存资料'}
      </Button>
      {#if saved}
        <span class="success" role="status">资料已保存</span>
      {/if}
    </div>
    {#if error}
      <p class="form-error" role="alert">{error}</p>
    {/if}
  </form>
</Card.Root>
