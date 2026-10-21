<script lang="ts">
  import { tick, untrack } from 'svelte';
  import { Plus, Search, X } from '@lucide/svelte';
  import { Combobox } from 'bits-ui';

  import * as Avatar from '#lib/components/ui/avatar/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Label } from '#lib/components/ui/label/index.js';

  import { useContacts } from './context.svelte';
  import type { User } from './api';
  import type { ContactsState } from './contacts.svelte';

  const SELECTION_LIMIT = 20;
  let {
    selected = $bindable<User[]>([]),
    excludedIds = [],
    disabled = false,
  }: {
    selected: User[];
    excludedIds?: string[];
    disabled?: boolean;
  } = $props();
  const contacts = useContacts();
  let query = $state('');
  let suggestionsOpen = $state(false);
  let search = $state.raw<ReturnType<ContactsState['watchSearch']>>();
  let composing = $state(false);
  const results = $derived(
    search?.result?.data?.pages.flatMap((page) =>
      page.items.map((item) => item.user),
    ) ?? [],
  );
  const nextCursor = $derived(search?.result?.hasNextPage);
  const searching = $derived(!search || search.result?.isFetching);
  const searchError = $derived(search?.result?.error?.message ?? '');
  const selectedIds = $derived(selected.map((user) => user.id));
  const available = $derived(
    results.filter(
      (user) =>
        !selectedIds.includes(user.id) && !excludedIds.includes(user.id),
    ),
  );

  async function selectUsers(ids: string[]) {
    const candidates = [...selected, ...results];
    selected = ids
      .flatMap((id) => {
        const user = candidates.find((candidate) => candidate.id === id);
        return user ? [user] : [];
      })
      .slice(0, SELECTION_LIMIT);
    suggestionsOpen = false;
    await tick();
    query = '';
  }

  function loadMore() {
    if (!searching) void search?.loadMore();
  }

  $effect(() => {
    const term = query.trim();
    const owner = contacts.owner;
    const enabled = !disabled && !composing && !!owner;
    search = undefined;
    if (!enabled) return;
    let current: ReturnType<ContactsState['watchSearch']> | undefined;
    const timer = setTimeout(() => {
      current = owner.watchSearch(term);
      search = current;
    }, 250);
    // Search identity alone owns the observer; cache notifications update its
    // results without resetting selected chips or subscribing this effect.
    return () => {
      clearTimeout(timer);
      untrack(() => current?.stop());
    };
  });
</script>

<Label for="peer-search">选择好友</Label>
<Combobox.Root
  type="multiple"
  value={selectedIds}
  onValueChange={selectUsers}
  inputValue={query}
  bind:open={suggestionsOpen}
  disabled={disabled || selected.length >= SELECTION_LIMIT}
>
  <div class="user-picker-field">
    {#each selected as user (user.id)}
      <span class="selected-user">
        <span title={`@${user.username}`}>{user.display_name}</span>
        <Button
          type="button"
          variant="ghost"
          size="icon-xs"
          aria-label={`移除 ${user.display_name} (@${user.username})`}
          {disabled}
          onclick={() => {
            selected = selected.filter((item) => item.id !== user.id);
          }}
        >
          <X size={12} />
        </Button>
      </span>
    {/each}
    <div class="user-picker-input">
      <Search size={18} aria-hidden="true" />
      <Combobox.Input
        id="peer-search"
        placeholder={selected.length ? '继续添加' : '输入用户名或昵称'}
        autocomplete="off"
        maxlength={64}
        oninput={(event) => {
          query = event.currentTarget.value;
          suggestionsOpen = true;
        }}
        oncompositionstart={() => (composing = true)}
        oncompositionend={() => (composing = false)}
      >
        {#snippet child({ props })}
          <Input
            {...props}
            class="h-9 border-0 bg-transparent px-0 shadow-none focus-visible:ring-0 dark:bg-transparent"
          />
        {/snippet}
      </Combobox.Input>
    </div>
  </div>
  <Combobox.ContentStatic class="user-search-results" aria-label="匹配好友">
    {#if searching}
      <p class="user-search-hint" role="status">正在搜索…</p>
    {:else if searchError}
      <p class="user-search-hint" role="alert">{searchError}</p>
      <Button type="button" variant="ghost" onclick={() => search?.retry()}>
        重试搜索
      </Button>
    {:else}
      {#each available as user (user.id)}
        <Combobox.Item
          class="user-search-option"
          value={user.id}
          label={`${user.display_name} @${user.username}`}
        >
          <Avatar.Root size="sm">
            <Avatar.Fallback>
              {Array.from(user.display_name)[0]}
            </Avatar.Fallback>
          </Avatar.Root>
          <span class="user-search-copy">
            <strong>{user.display_name}</strong>
            <span>@{user.username}</span>
          </span>
          <Plus size={16} aria-hidden="true" />
        </Combobox.Item>
      {:else}
        <p class="user-search-hint">
          {results.length ? '匹配好友已全部选择' : '没有找到匹配的好友'}
        </p>
      {/each}
      {#if nextCursor}
        <Button
          type="button"
          variant="ghost"
          disabled={searching}
          onclick={loadMore}
        >
          加载更多好友
        </Button>
      {/if}
    {/if}
  </Combobox.ContentStatic>
</Combobox.Root>
{#if selected.length >= SELECTION_LIMIT}
  <p class="field-help">最多选择 20 人</p>
{/if}
