<script lang="ts">
  import type { QueryObserverResult } from '@tanstack/svelte-query';
  import { useContacts } from '#lib/features/contacts/context.svelte.ts';
  import type { Relationship } from '#lib/features/contacts/api.ts';
  import AddFriendDialog from '#lib/features/contacts/AddFriendDialog.svelte';

  import { Send, Paperclip, X } from '@lucide/svelte';

  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Textarea } from '#lib/components/ui/textarea/index.js';

  import UploadList from '#lib/features/media/UploadList.svelte';

  import type { ChatSync } from './sync.svelte';
  import { membershipId, isGroup } from './models';

  const MAX_MESSAGE_CHARACTERS = 4_096;
  const CHARACTER_WARNING_THRESHOLD = 3_500;
  const MAX_COMPOSER_HEIGHT_PX = 160;

  let {
    sync,
    id,
  }: {
    sync: ChatSync;
    id: string;
  } = $props();

  const contacts = useContacts();
  let relationship = $state.raw<QueryObserverResult<Relationship>>();
  let addOpen = $state(false);
  const peer = $derived(
    sync.current && !isGroup(sync.current) ? sync.current.peer : undefined,
  );
  const allowed = $derived(
    isGroup(sync.current) ||
      (relationship?.data?.is_friend === true &&
        !relationship.isError &&
        !relationship.isFetching),
  );

  $effect(() => {
    const owner = contacts.owner;
    const userId = peer?.id;
    relationship = undefined;
    if (owner && userId)
      return owner.watchRelationship(userId, (value) => (relationship = value));
  });

  let composing = false;
  // Component refs are bindable props; their assignment participates in Svelte's
  // lifecycle even though the elements themselves do not own the message draft.
  let input = $state<HTMLTextAreaElement>(null!);
  let files = $state<HTMLInputElement>(null!);

  let draft = $derived(sync.drafts[id] ?? '');
  // Count Unicode scalars like Rust's chars(), rather than UTF-16 code units.
  let characterCount = $derived([...draft].length);
  const entries = $derived(sync.uploads.forConversation(id));
  const attachmentIds = $derived(sync.uploads.readyIds(id));
  const attachmentsReady = $derived(entries.length === attachmentIds.length);
  const canSend = $derived(
    allowed &&
      (!!draft.trim() || !!attachmentIds.length) &&
      attachmentsReady &&
      characterCount <= MAX_MESSAGE_CHARACTERS &&
      !sync.loading,
  );

  function resizeAndPublishTyping() {
    sync.drafts[id] = input.value;

    input.style.height = 'auto';
    input.style.height = `${Math.min(MAX_COMPOSER_HEIGHT_PX, input.scrollHeight)}px`;

    if (allowed) void sync.setTyping(id, !!input.value.trim());
  }

  function sendDraft() {
    if (composing || !canSend) {
      return;
    }

    const body = draft;
    sync.drafts[id] = '';
    input.style.height = 'auto';
    void sync.setTyping(id, false);
    void sync.send(body, undefined, attachmentIds);
    // Keep the existing explicit-send focus behavior; receiving a reply does not
    // call this handler or move focus into the composer.
    input.focus();
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Enter' && !event.shiftKey) {
      // Some IMEs still report keyCode 229 instead of isComposing. Enter must
      // finish composition before it can submit the user's text.
      if (event.isComposing || composing || event.keyCode === 229) {
        return;
      }
      event.preventDefault();
      sendDraft();
    }
  }
</script>

<div class="composer-wrap">
  {#if peer && !allowed}
    <div class="composer-hint" role="status">
      {#if relationship?.isError}
        <span>无法确认好友状态，暂时不能发送消息。</span>
        <Button
          variant="ghost"
          size="sm"
          onclick={() => contacts.owner?.refresh()}
        >
          重试
        </Button>
      {:else if !relationship?.data || relationship.isFetching}
        <span>正在确认好友状态…</span>
      {:else}
        <span>你们还不是好友，添加后即可发送消息。</span>
        <Button variant="ghost" size="sm" onclick={() => (addOpen = true)}>
          添加好友
        </Button>
      {/if}
    </div>
    <AddFriendDialog bind:open={addOpen} user={peer} />
  {/if}
  {#if sync.replyTarget}
    <div class="composer-reply" role="status">
      <div>
        <strong>
          回复 {sync.replyTarget.sender_name ??
            sync.senderName(sync.replyTarget.sender_id)}
        </strong>
        <span>
          {sync.replyTarget.withdrawn
            ? '此消息已撤回'
            : sync.replyTarget.body || '附件'}
        </span>
      </div>
      <Button
        type="button"
        variant="ghost"
        size="icon-sm"
        aria-label="取消回复"
        onclick={() => (sync.actions.replyId = null)}
      >
        <X size={14} />
      </Button>
    </div>
  {/if}
  {#if entries.length}
    <UploadList uploads={sync.uploads} {entries} />
  {/if}
  {#if sync.uploads.error}
    <p class="form-error" role="alert">
      {sync.uploads.error}
    </p>
  {/if}
  <Input
    type="file"
    multiple
    class="hidden"
    bind:ref={files}
    aria-label="选择附件"
    onchange={() => {
      const selected = Array.from(files.files ?? []);
      files.value = '';
      void sync.uploads.add(selected, {
        purpose: 'message',
        conversationId: id,
        membershipId: membershipId(sync.current),
      });
    }}
  />
  <form
    class="composer"
    onsubmit={(e) => {
      e.preventDefault();
      sendDraft();
    }}
  >
    <Button
      type="button"
      variant="ghost"
      size="icon"
      aria-label="添加附件"
      disabled={!allowed}
      onclick={() => files.click()}
    >
      <Paperclip size={19} />
    </Button>
    <Textarea
      bind:ref={input}
      class="min-h-0 field-sizing-fixed"
      value={draft}
      aria-label="输入消息"
      placeholder="写点什么…"
      rows={1}
      oninput={resizeAndPublishTyping}
      onkeydown={handleKeydown}
      oncompositionstart={() => (composing = true)}
      oncompositionend={() => (composing = false)}
    />
    <Button
      type="submit"
      size="icon"
      class="size-11 rounded-xl"
      aria-label="发送消息"
      disabled={!canSend}
    >
      <Send size={19} />
    </Button>
  </form>
  <div class="composer-hint">
    <span>Enter 发送 · Shift + Enter 换行</span>
    {#if characterCount > CHARACTER_WARNING_THRESHOLD}
      <span class:too-long={characterCount > MAX_MESSAGE_CHARACTERS}>
        {characterCount} / 4096
      </span>
    {/if}
  </div>
</div>
