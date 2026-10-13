<script lang="ts">
  import * as Card from '#lib/components/ui/card/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Label } from '#lib/components/ui/label/index.js';

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

<Card.Root class="settings-card gap-0 p-6" role="region" aria-label="头像">
  <div class="section-heading">
    <h2>头像</h2>
    <p class="muted">选择 JPEG、PNG 或 WebP 图片。</p>
  </div>
  <ProfileAvatar {userId} {name} {revision} />
  <Label class="form-label" for="avatar-file">选择头像</Label>
  <Input
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
    <Button disabled={busy || !ready} onclick={() => save(false)}>
      保存头像
    </Button>
    <Button variant="secondary" disabled={busy} onclick={() => save(true)}>
      移除头像
    </Button>
  </div>
  {#if error}<p class="form-error" role="alert">{error}</p>{/if}
</Card.Root>
