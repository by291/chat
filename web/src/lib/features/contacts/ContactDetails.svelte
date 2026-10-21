<script lang="ts">
  import { ArrowLeft, Users, UserPlus } from '@lucide/svelte';

  import { Button } from '#lib/components/ui/button/index.js';
  import * as Avatar from '#lib/components/ui/avatar/index.js';
  import PaneHeader from '#lib/components/PaneHeader.svelte';
  import { avatarTone } from '#lib/utils.js';

  import { useContacts, type ContactCategory } from './context.svelte';
  import type { User } from './api';
  import type { ContactRequest } from './requests/model';

  let {
    tab,
    selectedId,
    selectedUser,
    selectedRequest,
    revealing,
    busy,
    error,
    openChat,
    onback,
    onremove,
    onadd,
  }: {
    tab: ContactCategory;
    selectedId: string | null;
    selectedUser: User | undefined;
    selectedRequest: ContactRequest | undefined;
    revealing: boolean;
    busy: boolean;
    error: string;
    openChat: (user: User) => void;
    onback: () => void;
    onremove: (user: User) => void;
    onadd: () => void;
  } = $props();

  const contacts = useContacts();
</script>

{#if selectedId}
  <PaneHeader title={tab === 'friends' ? '好友资料' : '好友申请'}>
    {#snippet leading()}
      <Button
        variant="ghost"
        size="icon"
        class="contacts-back rounded-full size-10"
        aria-label="返回通讯录列表"
        onclick={onback}
      >
        <ArrowLeft class="size-5" />
      </Button>
    {/snippet}
  </PaneHeader>
  {#if selectedUser}
    <div class="contact-profile">
      <div class="contact-identity">
        <Avatar.Root
          class="size-20 rounded-2xl"
          data-tone={avatarTone(selectedUser.id)}
        >
          <Avatar.Fallback class="text-3xl">
            {Array.from(selectedUser.display_name)[0] ?? '…'}
          </Avatar.Fallback>
        </Avatar.Root>
        <div>
          <h3>{selectedUser.display_name}</h3>
          <p class="muted">@{selectedUser.username}</p>
        </div>
      </div>
      <dl class="contact-facts">
        <div>
          <dt>用户名</dt>
          <dd>@{selectedUser.username}</dd>
        </div>
        {#if selectedRequest}
          <div>
            <dt>申请时间</dt>
            <dd>{new Date(selectedRequest.created_at).toLocaleString()}</dd>
          </div>
        {/if}
      </dl>
      {#if tab !== 'friends'}
        <p class="contact-request-status">
          {selectedRequest?.direction === 'incoming'
            ? '对方请求添加你为好友'
            : '好友申请已发送，等待对方同意'}
        </p>
      {/if}
      {#if error || contacts.owner?.error}
        <p class="form-error" role="alert">
          {error || contacts.owner?.error}
        </p>
      {/if}
      <div class="contact-actions">
        {#if tab === 'friends'}
          <Button
            disabled={busy}
            onclick={() => selectedUser && openChat(selectedUser)}
          >
            发消息
          </Button>
          <Button
            variant="ghost"
            disabled={contacts.owner?.busy}
            onclick={() => selectedUser && onremove(selectedUser)}
          >
            删除好友
          </Button>
        {:else if selectedRequest}
          {#if selectedRequest.direction === 'incoming'}
            <Button
              disabled={contacts.owner?.busy}
              onclick={() =>
                selectedRequest &&
                contacts.owner?.respond(selectedRequest.id, 'accept')}
            >
              接受
            </Button>
            <Button
              variant="ghost"
              disabled={contacts.owner?.busy}
              onclick={() =>
                selectedRequest &&
                contacts.owner?.respond(selectedRequest.id, 'decline')}
            >
              拒绝
            </Button>
          {:else}
            <Button
              variant="secondary"
              disabled={contacts.owner?.busy}
              onclick={() =>
                selectedRequest &&
                contacts.owner?.respond(selectedRequest.id, 'cancel')}
            >
              取消申请
            </Button>
          {/if}
        {/if}
      </div>
    </div>
  {:else}
    <p class="contacts-feedback muted" role="status">
      {revealing ? '正在查找…' : '该记录已不存在或已处理'}
    </p>
  {/if}
{:else}
  <div class="contacts-placeholder">
    <Users size={48} aria-hidden="true" />
    <h2>{tab === 'friends' ? '和朋友保持联系' : '好友申请'}</h2>
    <p>
      {tab === 'friends'
        ? '选择一位好友，查看资料或开始聊天'
        : '选择一条申请，查看详情'}
    </p>
    {#if tab === 'friends'}
      <Button variant="secondary" onclick={onadd}>
        <UserPlus size={17} />添加好友
      </Button>
    {/if}
  </div>
{/if}
