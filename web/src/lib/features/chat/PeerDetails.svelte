<script lang="ts">
  import { Info } from '@lucide/svelte';
  import { Button } from '#lib/components/ui/button/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';
  import ProfileAvatar from '#lib/features/settings/profile/ProfileAvatar.svelte';
  import type { User } from '#lib/features/contacts/api.ts';

  let { user }: { user: User } = $props();
</script>

<Dialog.Root>
  <Dialog.Trigger aria-label="聊天资料">
    {#snippet child({ props })}<Button variant="ghost" size="icon" {...props}>
        <Info size={20} />
      </Button>{/snippet}
  </Dialog.Trigger>
  <Dialog.Content class="context-detail-panel" closeLabel="关闭资料">
    <Dialog.Header>
      <Dialog.Title>聊天资料</Dialog.Title>
      <Dialog.Description>当前聊天对象的个人信息</Dialog.Description>
    </Dialog.Header>
    <div class="own-profile-identity">
      <ProfileAvatar userId={user.id} name={user.display_name} />
      <h2>{user.display_name}</h2>
      <p class="muted">@{user.username}</p>
    </div>
    <dl class="profile-facts">
      <div>
        <dt>昵称</dt>
        <dd>{user.display_name}</dd>
      </div>
      <div>
        <dt>用户名</dt>
        <dd>{user.username}</dd>
      </div>
    </dl>
  </Dialog.Content>
</Dialog.Root>
