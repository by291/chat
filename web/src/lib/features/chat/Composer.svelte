<script lang="ts">
  import { Send, Paperclip, X } from '@lucide/svelte';

  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Textarea } from '#lib/components/ui/textarea/index.js';

  import UploadList from '#lib/features/media/UploadList.svelte';

  import type { ChatSync } from './sync.svelte';
  import { membershipId } from './models';

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
    (!!draft.trim() || !!attachmentIds.length) &&
      attachmentsReady &&
      characterCount <= MAX_MESSAGE_CHARACTERS &&
      !sync.loading,
  );

  function resizeAndPublishTyping() {
    sync.drafts[id] = input.value;

    input.style.height = 'auto';
    input.style.height = `${Math.min(MAX_COMPOSER_HEIGHT_PX, input.scrollHeight)}px`;

    void sync.setTyping(id, !!input.value.trim());
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
    {:else}
      <span>好好说话，慢慢联系。</span>
    {/if}
  </div>
</div>
