<script lang="ts">
  import { untrack } from 'svelte';
  import { Dialog } from 'bits-ui';
  import { Users, X } from '@lucide/svelte';
  import { goto } from '$app/navigation';
  import { page } from '$app/state';

  import { createGroupApi } from './api';
  import { isGroup, membershipId } from './models';
  import type { components } from '../../api-client';
  import type { ChatSync } from './sync.svelte';

  let { sync }: { sync: ChatSync } = $props();
  const requests = createGroupApi();
  let open = $state(false);
  let title = $state('');
  let busy = $state(false);
  let error = $state('');
  let invitation = $state<components['schemas']['GroupInvitationView'] | null>(
    null,
  );
  let lastIdentity: string | undefined;

  const group = $derived(isGroup(sync.current) ? sync.current : undefined);
  const manager = $derived(
    group?.my_role === 'owner' || group?.my_role === 'admin',
  );
  const roleNames = { owner: '群主', admin: '管理员', member: '成员' };
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
    invitation
      ? `${page.url.origin}/app/groups/join?invite=${invitation.token}`
      : '',
  );

  $effect(() => {
    const identity = membershipId(sync.current);
    // UI state is reset by membership identity, not ordinary roster/cache refreshes.
    untrack(() => {
      if (identity !== lastIdentity) {
        open = false;
        invitation = null;
        error = '';
        lastIdentity = identity;
      }
    });
  });

  async function mutate(
    operation: (id: string, membership: string) => Promise<unknown>,
  ) {
    if (!group || busy) return;
    const id = group.id;
    const identity = group.membership.membership_id;
    busy = true;
    error = '';
    try {
      await operation(id, identity);
      if (!sync.alive || membershipId(sync.current) !== identity) return;
      await sync.refreshSummaries();
      await sync.refreshMembers();
    } catch (reason) {
      error = reason instanceof Error ? reason.message : '群聊操作失败';
    } finally {
      busy = false;
    }
  }

  async function invite() {
    if (busy) return;
    // A previous single-use link must not remain copyable while a new one is pending.
    invitation = null;
    await mutate(async (id, identity) => {
      const response = await requests.invite(id, identity);
      if (sync.alive && membershipId(sync.current) === identity)
        invitation = response;
    });
  }

  async function copyLink() {
    try {
      await navigator.clipboard.writeText(link);
    } catch {
      error = '无法复制，请手动选择邀请链接';
    }
  }

  async function leave() {
    if (!group || busy) return;
    const identity = group.membership.membership_id;
    busy = true;
    try {
      await requests.leave(group.id, identity);
      if (
        !sync.alive ||
        (membershipId(sync.current) && membershipId(sync.current) !== identity)
      )
        return;
      await sync.refreshSummaries();
      open = false;
      await goto('/app');
    } catch (reason) {
      error = reason instanceof Error ? reason.message : '退出失败';
    } finally {
      busy = false;
    }
  }
</script>

{#if group}
  <Dialog.Root bind:open>
    <Dialog.Trigger
      class="icon-button"
      aria-label="群设置"
      onclick={() => {
        title = group?.title ?? '';
        error = '';
        void sync.refreshMembers();
      }}
    >
      <Users size={20} />
    </Dialog.Trigger>
    <Dialog.Portal>
      <Dialog.Overlay class="dialog-overlay" />
      <Dialog.Content class="dialog-content group-details">
        <Dialog.Close class="icon-button dialog-close" aria-label="关闭">
          <X size={20} />
        </Dialog.Close>
        <Dialog.Title class="dialog-title">{group.title}</Dialog.Title>
        <Dialog.Description class="muted">
          {group.member_count} 位成员 · 仅当前群成员可访问
        </Dialog.Description>
        {#if manager}
          <form
            onsubmit={(event) => {
              event.preventDefault();
              void mutate((id, identity) =>
                requests.rename(id, identity, title),
              );
            }}
          >
            <label for="rename-group">群名称</label>
            <input id="rename-group" bind:value={title} required />
            <button class="button subtle" disabled={busy || !title.trim()}>
              保存群名称
            </button>
          </form>
          <button class="button subtle" disabled={busy} onclick={invite}>
            生成邀请链接
          </button>
          {#if invitation}
            <label for="group-invite-link">
              邀请链接（单次使用，七天有效）
            </label>
            <input id="group-invite-link" value={link} readonly />
            <div class="group-actions">
              <button class="button subtle" onclick={copyLink}>复制链接</button>
              <button
                class="button subtle"
                disabled={busy}
                onclick={() =>
                  mutate(async (id, identity) => {
                    if (!invitation) return;
                    await requests.revoke(
                      id,
                      identity,
                      invitation.invitation_id,
                    );
                    invitation = null;
                  })}
              >
                撤销邀请
              </button>
            </div>
          {/if}
        {/if}
        <ul class="group-members" aria-label="群成员">
          {#each sync.groupMembers as member (member.membership_id)}
            <li>
              <div>
                <strong>{member.user.display_name}</strong>
                <small>
                  @{member.user.username} · {roleNames[member.role]}
                </small>
              </div>
              {#if group.my_role === 'owner' && member.role !== 'owner'}
                <button
                  class="button subtle compact"
                  disabled={busy}
                  onclick={() =>
                    mutate((id, identity) =>
                      requests.role(
                        id,
                        identity,
                        member.user.id,
                        member.role === 'admin' ? 'member' : 'admin',
                      ),
                    )}
                >
                  {member.role === 'admin' ? '取消管理员' : '设为管理员'}
                </button>
                <button
                  class="button subtle compact"
                  disabled={busy}
                  onclick={() =>
                    mutate((id, identity) =>
                      requests.transfer(id, identity, member.user.id),
                    )}
                >
                  转让群主
                </button>
              {/if}
              {#if manager && member.user.id !== sync.userId && member.role !== 'owner' && (group.my_role === 'owner' || member.role === 'member')}
                <button
                  class="button subtle compact"
                  disabled={busy}
                  onclick={() =>
                    mutate((id, identity) =>
                      requests.remove(id, identity, member.user.id),
                    )}
                >
                  移除
                </button>
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
        <button
          class="button subtle"
          disabled={busy || group.my_role === 'owner'}
          onclick={leave}
        >
          退出群聊
        </button>
        {#if group.my_role === 'owner'}
          <p class="muted">群主需要先转让群主身份，才能退出。</p>
        {/if}
        {#if error}
          <p class="form-error" role="alert">{error}</p>
        {/if}
      </Dialog.Content>
    </Dialog.Portal>
  </Dialog.Root>
{/if}
