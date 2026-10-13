<script lang="ts">
  import { Send, Paperclip } from '@lucide/svelte';
  import type { ChatSync } from './sync.svelte';
  import { membershipId } from './models';
  import UploadList from '../media/UploadList.svelte';

  let {
    sync,
    id,
  }: {
    sync: ChatSync;
    id: string;
  } = $props();

  let composing = false;
  let input: HTMLTextAreaElement;
  let files: HTMLInputElement;

  let draft = $derived(sync.drafts[id] ?? '');
  // Count Unicode scalars like Rust's chars(), rather than UTF-16 code units.
  let characterCount = $derived([...draft].length);
  const entries = $derived(sync.uploads.forConversation(id));
  const attachmentIds = $derived(sync.uploads.readyIds(id));
  const attachmentsReady = $derived(entries.length === attachmentIds.length);

  function resizeAndPublishTyping() {
    sync.drafts[id] = input.value;
    input.style.height = 'auto';
    input.style.height = `${Math.min(160, input.scrollHeight)}px`;
    void sync.setTyping(id, !!input.value.trim());
  }

  function sendDraft() {
    if (
      composing ||
      (!draft.trim() && !attachmentIds.length) ||
      !attachmentsReady ||
      characterCount > 4096 ||
      sync.loading
    ) {
      return;
    }
    const body = draft;
    sync.drafts[id] = '';
    input.style.height = 'auto';
    void sync.setTyping(id, false);
    void sync.send(body, undefined, attachmentIds);
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
  {#if entries.length}<UploadList uploads={sync.uploads} {entries} />{/if}
  {#if sync.uploads.error}<p class="form-error" role="alert">
      {sync.uploads.error}
    </p>{/if}
  <input
    type="file"
    multiple
    class="visually-hidden"
    bind:this={files}
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
    <button
      type="button"
      class="icon-button"
      aria-label="添加附件"
      onclick={() => files.click()}
    >
      <Paperclip size={19} />
    </button>
    <textarea
      bind:this={input}
      value={draft}
      aria-label="输入消息"
      placeholder="写点什么…"
      rows="1"
      oninput={resizeAndPublishTyping}
      onkeydown={handleKeydown}
      oncompositionstart={() => (composing = true)}
      oncompositionend={() => (composing = false)}></textarea>
    <button
      class="send-button"
      aria-label="发送消息"
      disabled={(!draft.trim() && !attachmentIds.length) ||
        !attachmentsReady ||
        characterCount > 4096 ||
        sync.loading}
    >
      <Send size={19} />
    </button>
  </form>
  <div class="composer-hint">
    <span>Enter 发送 · Shift + Enter 换行</span>
    {#if characterCount > 3500}
      <span class:too-long={characterCount > 4096}>
        {characterCount} / 4096
      </span>
    {:else}
      <span>好好说话，慢慢联系。</span>
    {/if}
  </div>
</div>
