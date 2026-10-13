<script lang="ts">
  import { useChat } from '../chat/context.svelte';
  import UploadList from '../media/UploadList.svelte';
  import ProfileAvatar from './ProfileAvatar.svelte';
  import { settingsApi } from './api';

  let { userId, name }: { userId: string; name: string } = $props();
  const chat = useChat();
  let revision = $state(0);
  let busy = $state(false);
  let error = $state('');
  const uploads = $derived(chat.sync?.uploads);
  const entries = $derived(
    uploads?.entries.filter((entry) => entry.scope.purpose === 'avatar') ?? [],
  );
  const ready = $derived(
    entries.find(
      (entry) =>
        uploads?.metadata(entry)?.state === 'ready' &&
        !entry.working &&
        !entry.error,
    ),
  );

  async function save(clear: boolean) {
    if (busy || (!clear && !ready?.attachmentId)) return;
    const scope = chat.sync;
    busy = true;
    error = '';
    try {
      if (clear) await settingsApi.clearAvatar();
      else await settingsApi.setAvatar(ready!.attachmentId!);
      if (!scope?.alive || chat.sync !== scope) return;
      if (ready?.attachmentId) uploads?.consume([ready.attachmentId]);
      revision++;
    } catch (reason) {
      if (scope?.alive && chat.sync === scope)
        error = reason instanceof Error ? reason.message : '头像保存失败';
    } finally {
      busy = false;
    }
  }
</script>

<section class="settings-card">
  <div class="section-heading">
    <h2>头像</h2>
    <p class="muted">选择 JPEG、PNG 或 WebP 图片。</p>
  </div>
  <ProfileAvatar {userId} {name} {revision} />
  <label for="avatar-file">选择头像</label>
  <input
    id="avatar-file"
    type="file"
    accept="image/jpeg,image/png,image/webp"
    disabled={busy || entries.length > 0}
    onchange={(event) => {
      const selected = event.currentTarget.files?.[0];
      event.currentTarget.value = '';
      if (selected) void uploads?.add([selected], { purpose: 'avatar' });
    }}
  />
  {#if uploads && entries.length}<UploadList {uploads} {entries} />{/if}
  <div class="form-actions">
    <button
      class="button primary"
      disabled={busy || !ready}
      onclick={() => save(false)}
    >
      保存头像
    </button>
    <button class="button subtle" disabled={busy} onclick={() => save(true)}>
      移除头像
    </button>
  </div>
  {#if error}<p class="form-error" role="alert">{error}</p>{/if}
</section>
