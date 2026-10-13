<script lang="ts">
  import { untrack } from 'svelte';
  import type { components } from '../../api-client';
  import { settingsApi } from './api';

  let {
    user,
    onSaved,
  }: {
    user: components['schemas']['UserView'];
    onSaved: (user: components['schemas']['UserView']) => void;
  } = $props();

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

<section class="settings-card">
  <div class="section-heading">
    <span class="eyebrow">PROFILE</span>
    <h2>个人资料</h2>
    <p class="muted">让朋友更容易认出你。</p>
  </div>
  <form onsubmit={submit}>
    <label for="profile-username">用户名</label>
    <input id="profile-username" value={`@${user.username}`} readonly />
    <p class="field-help">用户名是你的唯一标识。</p>
    <label for="profile-name">昵称</label>
    <input id="profile-name" bind:value={displayName} required />
    <div class="form-actions">
      <button class="button primary" disabled={pending}>
        {pending ? '保存中…' : '保存资料'}
      </button>
      {#if saved}
        <span class="success" role="status">资料已保存</span>
      {/if}
    </div>
    {#if error}
      <p class="form-error" role="alert">{error}</p>
    {/if}
  </form>
</section>
