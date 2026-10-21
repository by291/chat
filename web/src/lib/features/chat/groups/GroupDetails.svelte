<script lang="ts">
  import { untrack } from 'svelte';
  import { Users } from '@lucide/svelte';
  import { goto } from '$app/navigation';
  import { page } from '$app/state';

  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Label } from '#lib/components/ui/label/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';

  import AddGroupMembersDialog from '#lib/features/contacts/AddGroupMembersDialog.svelte';

  import {
    GroupActions,
    canManageMemberRole,
    canRemoveGroupMember,
  } from './group-actions.svelte';
  import { isGroup, membershipId } from '../models';
  import type { ChatSync } from '../sync.svelte';

  const roleNames = { owner: '群主', admin: '管理员', member: '成员' };

  let { sync }: { sync: ChatSync } = $props();

  let open = $state(false);
  let addOpen = $state(false);
  let title = $state('');
  let lastIdentity: string | undefined;

  const group = $derived(isGroup(sync.current) ? sync.current : undefined);
  const actions = new GroupActions({
    group: () => group,
    membership: () => membershipId(sync.current),
    active: () => sync.alive,
    refreshSummaries: () => sync.refreshSummaries(),
    refreshMembers: () => sync.refreshMembers(),
  });
  const manager = $derived(
    group?.my_role === 'owner' || group?.my_role === 'admin',
  );
  const latestOwnMessage = $derived(
    sync.messages.findLast((message) => message.sender_id === sync.userId),
  );
  const readerNames = $derived(
    latestOwnMessage
      ? sync
          .readers(latestOwnMessage.created_seq)
          .map((member) => member.user.display_name)
      : [],
  );
  const link = $derived(
    actions.invitation
      ? `${page.url.origin}/app/groups/join?invite=${actions.invitation.token}`
      : '',
  );

  async function copyLink() {
    try {
      await navigator.clipboard.writeText(link);
    } catch {
      actions.error = '无法复制，请手动选择邀请链接';
    }
  }

  async function leave() {
    await actions.leave(async () => {
      open = false;
      await goto('/app');
    });
  }

  $effect(() => {
    const identity = membershipId(sync.current);
    // UI state is reset by membership identity, not ordinary roster/cache refreshes.
    untrack(() => {
      if (identity !== lastIdentity) {
        open = false;
        addOpen = false;
        actions.resetPresentation();
        lastIdentity = identity;
      }
    });
  });
</script>

{#if group}
  <Dialog.Root bind:open>
    <Dialog.Trigger
      aria-label="群设置"
      onclick={() => {
        title = group?.title ?? '';
        actions.error = '';
        void sync.refreshMembers();
      }}
    >
      {#snippet child({ props })}
        <Button variant="ghost" size="icon" {...props}>
          <Users size={20} />
        </Button>
      {/snippet}
    </Dialog.Trigger>
    <Dialog.Content
      class="group-details context-detail-panel"
      closeLabel="关闭"
    >
      <Dialog.Title>{group.title}</Dialog.Title>
      <Dialog.Description>
        {group.member_count} 位成员 · 仅当前群成员可访问
      </Dialog.Description>
      {#if manager}
        <form
          onsubmit={(event) => {
            event.preventDefault();
            void actions.rename(title);
          }}
        >
          <Label class="form-label" for="rename-group">群名称</Label>
          <Input id="rename-group" bind:value={title} required />
          <Button
            type="submit"
            variant="secondary"
            disabled={actions.busy || !title.trim()}
          >
            保存群名称
          </Button>
        </form>
        <Button
          variant="secondary"
          disabled={actions.busy}
          onclick={() => actions.invite()}
        >
          生成邀请链接
        </Button>
        {#if actions.invitation}
          <Label class="form-label" for="group-invite-link">
            邀请链接（单次使用，七天有效）
          </Label>
          <Input id="group-invite-link" value={link} readonly />
          <div class="group-actions">
            <Button variant="secondary" onclick={copyLink}>复制链接</Button>
            <Button
              variant="secondary"
              disabled={actions.busy}
              onclick={() => actions.revokeInvitation()}
            >
              撤销邀请
            </Button>
          </div>
        {/if}
      {/if}
      <Button variant="secondary" onclick={() => (addOpen = true)}>
        添加成员
      </Button>
      <AddGroupMembersDialog {sync} bind:open={addOpen} />
      <ul class="group-members" aria-label="群成员">
        {#each sync.groupMembers as member (member.membership_id)}
          <li>
            <div>
              <strong>{member.user.display_name}</strong>
              <small>
                @{member.user.username} · {roleNames[member.role]}
              </small>
            </div>
            {#if canManageMemberRole(group, member)}
              <Button
                variant="secondary"
                size="sm"
                disabled={actions.busy}
                onclick={() =>
                  actions.setRole(
                    member.user.id,
                    member.role === 'admin' ? 'member' : 'admin',
                  )}
              >
                {member.role === 'admin' ? '取消管理员' : '设为管理员'}
              </Button>
              <Button
                variant="secondary"
                size="sm"
                disabled={actions.busy}
                onclick={() => actions.transferOwnership(member.user.id)}
              >
                转让群主
              </Button>
            {/if}
            {#if canRemoveGroupMember(group, member, sync.userId)}
              <Button
                variant="secondary"
                size="sm"
                disabled={actions.busy}
                onclick={() => actions.removeMember(member.user.id)}
              >
                移除
              </Button>
            {/if}
          </li>
        {/each}
      </ul>
      {#if latestOwnMessage}
        <p class="muted" aria-label="消息已读成员">
          最近一条已发送消息已读：{readerNames.length
            ? readerNames.join('、')
            : '暂无'}
        </p>
      {/if}
      <Button
        variant="secondary"
        disabled={actions.busy || group.my_role === 'owner'}
        onclick={leave}
      >
        退出群聊
      </Button>
      {#if group.my_role === 'owner'}
        <p class="muted">群主需要先转让群主身份，才能退出。</p>
      {/if}
      {#if actions.error}
        <p class="form-error" role="alert">{actions.error}</p>
      {/if}
    </Dialog.Content>
  </Dialog.Root>
{/if}
