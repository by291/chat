<script lang="ts">
  import { FileText, Image, Users } from '@lucide/svelte';

  import * as Avatar from '#lib/components/ui/avatar/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { avatarTone } from '#lib/utils.js';
  import ProfileAvatar from '#lib/features/settings/profile/ProfileAvatar.svelte';
  import AttachmentList from '#lib/features/media/AttachmentList.svelte';
  import { conversationName, isGroup } from './models';
  import type { ChatSync } from './sync.svelte';

  let { sync }: { sync: ChatSync } = $props();
  const current = $derived(sync.current);
  // Only summarize loaded canonical history; this is not a second media index.
  const attachments = $derived(
    sync.messages
      .filter((message) => !message.withdrawn)
      .flatMap((message) => message.attachments ?? [])
      .slice(-4)
      .reverse(),
  );
</script>

<aside
  id="conversation-overview"
  class="conversation-overview"
  aria-label="聊天概览"
>
  <div class="overview-heading"><span>聊天资料</span></div>
  <div class="overview-identity">
    {#if current && !isGroup(current)}
      <ProfileAvatar
        userId={current.peer.id}
        name={current.peer.display_name}
      />
    {:else}
      <Avatar.Root data-tone={avatarTone(current?.id ?? '')}>
        <Avatar.Fallback><Users size={26} /></Avatar.Fallback>
      </Avatar.Root>
    {/if}
    <h2>{conversationName(current)}</h2>
    <p>
      {isGroup(current)
        ? `${current.member_count} 位成员`
        : current
          ? `@${current.peer.username}`
          : ''}
    </p>
  </div>
  <section class="overview-section">
    <h3>关于这段对话</h3>
    <dl>
      <div>
        <dt>类型</dt>
        <dd>{isGroup(current) ? '群聊' : '私聊'}</dd>
      </div>
      <div>
        <dt>消息提醒</dt>
        <dd>{current?.preferences.muted ? '已静音' : '已开启'}</dd>
      </div>
      <div>
        <dt>会话置顶</dt>
        <dd>{current?.preferences.pinned ? '已置顶' : '未置顶'}</dd>
      </div>
    </dl>
  </section>
  {#if isGroup(current) && sync.groupMembers.length}
    <section class="overview-section">
      <h3><Users size={15} />群成员</h3>
      <div class="overview-members">
        {#each sync.groupMembers.slice(0, 6) as member (member.user.id)}
          <span title={member.user.display_name}>
            <Avatar.Root data-tone={avatarTone(member.user.id)}>
              <Avatar.Fallback>
                {Array.from(member.user.display_name)[0]}
              </Avatar.Fallback>
            </Avatar.Root>
          </span>
        {/each}
      </div>
      <p class="overview-note">在上方“群设置”中查看和管理全部成员。</p>
    </section>
  {/if}
  <section class="overview-section overview-media">
    <h3><Image size={15} />图片与文件</h3>
    <p class="overview-note">当前已加载消息中的最近附件</p>
    {#each attachments as attachment (attachment.id)}
      <AttachmentList {attachment} />
    {:else}
      <div class="overview-empty">
        <FileText size={25} />
        <span>还没有附件</span>
      </div>
    {/each}
  </section>
  <Button variant="ghost" href="/app/contacts" class="overview-contacts">
    <Users size={16} />打开通讯录
  </Button>
</aside>

<style>
  .conversation-overview {
    background: var(--detail-surface);
    min-height: 0;
    overflow: auto;
    padding: 0 20px 20px;
    font-size: 13px;
  }
  .overview-heading {
    height: var(--workspace-header-height);
    display: flex;
    align-items: center;
    color: var(--subtle-text);
    font-size: 12px;
  }
  .overview-identity {
    display: flex;
    align-items: center;
    flex-direction: column;
    gap: 8px;
    padding: 16px 0 28px;
    text-align: center;
  }
  .overview-identity :global([data-slot='avatar']) {
    width: 64px;
    height: 64px;
    border-radius: 22px;
    font-size: 24px;
    margin-bottom: 6px;
  }
  .overview-identity h2 {
    font-size: 17px;
    overflow-wrap: anywhere;
    letter-spacing: 0;
  }
  .overview-identity p {
    color: var(--subtle-text);
    font-size: 12px;
    overflow-wrap: anywhere;
  }
  .overview-section {
    padding: 20px 0;
    border-top: 1px solid var(--line);
  }
  .overview-section h3 {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    font-weight: 500;
    margin: 0 0 14px;
  }
  dl {
    margin: 0;
    display: grid;
    gap: 14px;
  }
  dl > div {
    display: flex;
    justify-content: space-between;
    gap: 10px;
  }
  dt,
  .overview-note {
    color: var(--subtle-text);
    font-size: 12px;
  }
  dd {
    margin: 0;
  }
  .overview-members {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-bottom: 12px;
  }
  .overview-members :global([data-slot='avatar']) {
    width: 32px;
    height: 32px;
    border-radius: 10px;
  }
  .overview-note {
    line-height: 1.7;
  }
  .overview-empty {
    display: grid;
    justify-items: center;
    gap: 10px;
    padding: 28px 0;
    color: var(--subtle-text);
    font-size: 12px;
  }
  .overview-media :global(.attachment) {
    margin-top: 12px;
  }
  :global(.overview-contacts) {
    width: 100%;
  }
  @media (max-width: 1180px) {
    .conversation-overview {
      display: none;
    }
  }
</style>
