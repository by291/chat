<script lang="ts">
  import { Check, CheckCheck } from '@lucide/svelte';

  import { Button } from '#lib/components/ui/button/index.js';

  import AttachmentList from '#lib/features/media/AttachmentList.svelte';

  import type { Message } from './models';
  import { isGroup, sequence } from './models';
  import type { ChatSync } from './sync.svelte';
  import MessageReactions from './MessageReactions.svelte';

  let {
    sync,
    message,
    onLayout,
  }: { sync: ChatSync; message: Message; onLayout: () => void } = $props();

  const own = $derived(message.sender_id === sync.userId);
  const edited = $derived(
    // Reactions advance the projection revision without editing the message body.
    !message.withdrawn && !!message.edited_at,
  );
  const replyName = $derived(
    message.reply?.sender_name ??
      (message.reply ? sync.senderName(message.reply.sender_id) : '原消息'),
  );
  const replyBody = $derived(
    !message.reply
      ? '原消息不可用'
      : message.reply.withdrawn
        ? '此消息已撤回'
        : message.reply.body || '附件',
  );
  const receipt = $derived.by(() => {
    if (!own) return null;
    if (isGroup(sync.current)) {
      if (!sync.groupMembers.length) return null;
      return {
        label: `${sync.readCount(message.created_seq)} 人已读`,
        kind: 'group',
      };
    }
    return sequence(sync.current?.peer_read_seq ?? '0') >=
      sequence(message.created_seq)
      ? { label: '已读', kind: 'read' }
      : { label: '已保存', kind: 'saved' };
  });

  const time = (value: string) =>
    new Date(value).toLocaleTimeString('zh-CN', {
      hour: '2-digit',
      minute: '2-digit',
    });
</script>

<div class="message-bubble" class:withdrawn={message.withdrawn}>
  {#if isGroup(sync.current)}
    <strong class="message-sender">
      {message.sender_name ?? sync.senderName(message.sender_id)}
    </strong>
  {/if}
  {#if message.withdrawn}
    <p class="message-tombstone">此消息已撤回</p>
  {:else}
    {#if message.reply_to_message_id}
      <Button
        type="button"
        variant="ghost"
        class="reply-preview"
        disabled={!message.reply}
        aria-label="跳转到原消息"
        onclick={() => sync.history.jump(message.reply_to_message_id!)}
      >
        <strong>{replyName}</strong>
        <span>{replyBody}</span>
      </Button>
    {/if}
    {#if message.body}
      <div class="message-content">
        <p class="message-body">{message.body}</p>
      </div>
    {/if}
    {#each message.attachments ?? [] as attachment (attachment.id)}
      <AttachmentList {attachment} {onLayout} />
    {/each}
    <div class="message-actions" aria-label="消息操作">
      <Button
        type="button"
        variant="ghost"
        size="xs"
        onclick={() => sync.actions.reply(message)}
      >
        回复
      </Button>
      {#if own}
        <Button
          type="button"
          variant="ghost"
          size="xs"
          onclick={() => sync.actions.edit(message)}
        >
          编辑
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="xs"
          onclick={() => (sync.actions.withdrawingId = message.id)}
        >
          撤回
        </Button>
      {/if}
    </div>
    <MessageReactions {message} actions={sync.actions} />
  {/if}
  <div class="message-meta">
    {#if edited}
      <div class="message-edited">已编辑</div>
    {/if}
    <time datetime={message.created_at}>{time(message.created_at)}</time>
    {#if receipt}
      <div class="message-receipt" aria-label={receipt.label}>
        {#if receipt.kind === 'read'}
          <CheckCheck size={13} />
        {:else if receipt.kind === 'saved'}
          <Check size={13} />
        {:else}
          {receipt.label}
        {/if}
      </div>
    {/if}
  </div>
</div>
