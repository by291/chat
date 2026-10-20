<script lang="ts">
  import { untrack } from 'svelte';

  import { Button } from '#lib/components/ui/button/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';
  import { auth } from '#lib/features/auth/session.svelte.ts';

  import ProfileAvatar from './ProfileAvatar.svelte';
  import ProfileForm from './ProfileForm.svelte';
  import AvatarEditor from './AvatarEditor.svelte';

  let {
    open = $bindable(false),
    onCloseAutoFocus,
  }: {
    open: boolean;
    onCloseAutoFocus: (event: Event) => void;
  } = $props();
  let editing = $state(false);
  let saved = $state(false);
  let revision = $state(0);

  $effect(() => {
    if (open)
      untrack(() => {
        editing = false;
        saved = false;
      });
  });
</script>

<Dialog.Root bind:open>
  <Dialog.Content
    class="own-profile-dialog"
    closeLabel="关闭"
    {onCloseAutoFocus}
  >
    <Dialog.Header>
      <Dialog.Title>个人资料</Dialog.Title>
      <Dialog.Description>
        {editing ? '编辑你的昵称和头像。' : '朋友看到的个人信息。'}
      </Dialog.Description>
    </Dialog.Header>
    {#if auth.user}
      {#if editing}
        <ProfileForm
          user={auth.user}
          onSaved={(user) => {
            // The mounted authenticated identity owns this mutation result.
            if (auth.user?.id !== user.id) return;
            auth.user = user;
            editing = false;
            saved = true;
            revision++;
          }}
          onCancel={() => {
            editing = false;
            revision++;
          }}
        />
        <AvatarEditor userId={auth.user.id} name={auth.user.display_name} />
      {:else}
        <div class="own-profile-identity">
          <ProfileAvatar
            userId={auth.user.id}
            name={auth.user.display_name}
            {revision}
          />
          <h2>{auth.user.display_name}</h2>
          <p class="muted">@{auth.user.username}</p>
        </div>
        <dl class="profile-facts">
          <div>
            <dt>昵称</dt>
            <dd>{auth.user.display_name}</dd>
          </div>
          <div>
            <dt>用户名</dt>
            <dd>{auth.user.username}</dd>
          </div>
        </dl>
        <Dialog.Footer>
          <Button onclick={() => (editing = true)}>编辑资料</Button>
        </Dialog.Footer>
        {#if saved}<p class="success" role="status">资料已保存</p>{/if}
      {/if}
    {/if}
  </Dialog.Content>
</Dialog.Root>
