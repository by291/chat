<script lang="ts">
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';

  import { Search, Settings2 } from '@lucide/svelte';
  import type { ChatSync } from './sync.svelte';

  let { sync }: { sync: ChatSync } = $props();
  let searchOpen = $state(false);
  let settingsOpen = $state(false);

  const showEmptyResults = $derived(
    sync.history.hasSearched &&
      !sync.history.searching &&
      !sync.searchResults.length,
  );
  const hasMoreResults = $derived(sync.history.hasMoreResults);

  async function jump(id: string) {
    searchOpen = false;
    await sync.history.jump(id);
  }
</script>

<div class="conversation-tools">
  <Dialog.Root bind:open={searchOpen}>
    <Dialog.Trigger
      class="inline-flex size-9 items-center justify-center rounded-md hover:bg-accent"
      aria-label="搜索聊天记录"
    >
      <Search size={18} />
    </Dialog.Trigger>
    <Dialog.Content class="search-dialog" closeLabel="关闭">
      <Dialog.Header>
        <Dialog.Title>搜索聊天记录</Dialog.Title>
        <Dialog.Description>查找此会话中的文字，支持中文。</Dialog.Description>
      </Dialog.Header>
      <form
        class="message-search-form"
        onsubmit={(event) => {
          event.preventDefault();
          void sync.history.search();
        }}
      >
        <Input
          aria-label="搜索消息"
          placeholder="输入关键词"
          bind:value={sync.history.query}
        />
        <Button
          type="submit"
          disabled={sync.history.searching ||
            !sync.history.query.trim() ||
            [...sync.history.query.trim()].length > 200}
        >
          搜索
        </Button>
      </form>
      <div
        class="message-search-results"
        aria-label="消息搜索结果"
        aria-busy={sync.history.searching}
      >
        {#each sync.searchResults as message (message.id)}
          <Button
            type="button"
            variant="ghost"
            class="search-result"
            onclick={() => jump(message.id)}
          >
            <strong>
              {message.sender_name ?? sync.senderName(message.sender_id)}
            </strong>
            <span>{message.body}</span>
            <time datetime={message.created_at}>
              {new Date(message.created_at).toLocaleString('zh-CN')}
            </time>
          </Button>
        {/each}
        {#if showEmptyResults}
          <div class="search-empty"><p>没有找到匹配的消息</p></div>
        {/if}
      </div>
      {#if hasMoreResults}
        <div class="grid">
          <Button
            type="button"
            variant="outline"
            disabled={sync.history.searching}
            onclick={() => sync.history.search(true)}
          >
            加载更多结果
          </Button>
        </div>
      {/if}
    </Dialog.Content>
  </Dialog.Root>
  <Dialog.Root bind:open={settingsOpen}>
    <Dialog.Trigger
      class="inline-flex size-9 items-center justify-center rounded-md hover:bg-accent"
      aria-label="会话设置"
    >
      <Settings2 size={18} />
    </Dialog.Trigger>
    <Dialog.Content closeLabel="关闭">
      <Dialog.Header>
        <Dialog.Title>会话设置</Dialog.Title>
        <Dialog.Description>
          仅影响你的账号，并同步到你的其他设备。
        </Dialog.Description>
      </Dialog.Header>
      {#if sync.current}
        <div class="conversation-preferences">
          <Button
            type="button"
            variant="outline"
            aria-pressed={sync.current.preferences.pinned}
            disabled={sync.actions.busy}
            onclick={() =>
              sync.actions.preferences({
                pinned: !sync.current!.preferences.pinned,
              })}
          >
            {sync.current.preferences.pinned ? '取消置顶' : '置顶会话'}
          </Button>
          <Button
            type="button"
            variant="outline"
            aria-pressed={sync.current.preferences.muted}
            disabled={sync.actions.busy}
            onclick={() =>
              sync.actions.preferences({
                muted: !sync.current!.preferences.muted,
              })}
          >
            {sync.current.preferences.muted ? '取消静音' : '静音会话'}
          </Button>
          <Button
            type="button"
            variant="outline"
            aria-pressed={sync.current.preferences.archived}
            disabled={sync.actions.busy}
            onclick={() =>
              sync.actions.preferences({
                archived: !sync.current!.preferences.archived,
              })}
          >
            {sync.current.preferences.archived ? '移出归档' : '归档会话'}
          </Button>
        </div>
      {/if}
    </Dialog.Content>
  </Dialog.Root>
</div>
