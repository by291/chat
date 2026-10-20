<script lang="ts">
  import { onDestroy, onMount, tick, untrack } from 'svelte';
  import { goto } from '$app/navigation';
  import { page } from '$app/state';
  import {
    ArrowLeft,
    ChevronDown,
    Search,
    UserPlus,
    Users,
  } from '@lucide/svelte';

  import { Input } from '#lib/components/ui/input/index.js';
  import * as DropdownMenu from '#lib/components/ui/dropdown-menu/index.js';
  import AccountMenu from '#lib/features/settings/AccountMenu.svelte';
  import type { ContactsState } from './contacts.svelte';

  import { Button } from '#lib/components/ui/button/index.js';
  import * as Avatar from '#lib/components/ui/avatar/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';
  import { startDirectConversation } from '#lib/features/chat/api.ts';
  import { useChat } from '#lib/features/chat/context.svelte.ts';

  import PaneHeader from '#lib/components/PaneHeader.svelte';
  import { avatarTone } from '#lib/utils.js';

  import AddFriendDialog from './AddFriendDialog.svelte';
  import { useContacts, type ContactCategory } from './context.svelte';
  import type { User } from './api';
  import './contacts.css';

  const contacts = useContacts();
  const chat = useChat();
  const lifetime = new AbortController();
  const tabs = [
    { id: 'friends', label: '好友' },
    { id: 'incoming', label: '新的朋友' },
    { id: 'outgoing', label: '已发送' },
  ] as const;

  let addOpen = $state(false);
  let removing = $state<User>();
  let busy = $state(false);
  let error = $state('');
  let query = $state('');
  let composing = $state(false);
  let revealing = $state(false);
  let searchingRequests = $state(false);
  let search = $state.raw<ReturnType<ContactsState['watchSearch']>>();
  let list: HTMLElement;
  let detailPanel = $state<HTMLElement>();
  const tab = $derived(
    (tabs.find((item) => item.id === page.url.searchParams.get('category'))
      ?.id ?? 'friends') as ContactCategory,
  );
  const selectedId = $derived(page.url.searchParams.get('person'));

  // The URL owns selection, including browser Back/Forward. Context only remembers
  // the last destination for the main navigation and the list's reading position.
  $effect(() => {
    const category = tab;
    const id = selectedId;
    untrack(() => {
      if (contacts.view.category !== category) {
        contacts.view.scrollTop = 0;
        if (list) list.scrollTop = 0;
      }
      contacts.view.category = category;
      contacts.view.selectedId = id;
    });
  });
  const requests = $derived(
    (tab === 'incoming'
      ? contacts.owner?.incoming
      : contacts.owner?.outgoing) ?? [],
  );
  const selectedRequest = $derived(
    requests.find((request) => request.id === selectedId),
  );
  const selectedUser = $derived(
    tab === 'friends'
      ? (contacts.owner?.friends.find((friend) => friend.user.id === selectedId)
          ?.user ??
          search?.result?.data?.pages
            .flatMap((page) => page.items)
            .find((friend) => friend.user.id === selectedId)?.user)
      : tab === 'incoming'
        ? selectedRequest?.sender
        : selectedRequest?.recipient,
  );

  const term = $derived(query.trim());
  const friends = $derived(
    term
      ? (search?.result?.data?.pages.flatMap((page) => page.items) ?? [])
      : (contacts.owner?.friends ?? []),
  );
  const filteredRequests = $derived(
    requests.filter((request) => {
      const user = tab === 'incoming' ? request.sender : request.recipient;
      return `${user.display_name} ${user.username}`
        .toLocaleLowerCase()
        .includes(term.toLocaleLowerCase());
    }),
  );
  const searching = $derived(
    tab === 'friends' && !!term
      ? !search || search.result?.isFetching
      : searchingRequests,
  );
  const listError = $derived(
    search?.result?.error?.message || contacts.owner?.error,
  );
  const hasMore = $derived(
    tab === 'friends' && term
      ? search?.result?.hasNextPage
      : contacts.owner?.hasMore(tab),
  );

  $effect(() => {
    const owner = contacts.owner;
    const category = tab;
    const id = selectedId;
    revealing = !!owner && !!id;
    if (!owner || !id) return;
    const controller = new AbortController();
    // URL identity owns this read; cache updates must not restart pagination.
    untrack(
      () =>
        void owner.reveal(category, id, controller.signal).finally(() => {
          if (!controller.signal.aborted) revealing = false;
        }),
    );
    return () => controller.abort();
  });

  $effect(() => {
    const owner = contacts.owner;
    const category = tab;
    const value = term;
    const enabled = !composing && !!owner && !!value;
    search = undefined;
    searchingRequests = enabled && category !== 'friends';
    if (!enabled) return;
    const controller = new AbortController();
    let observer: ReturnType<ContactsState['watchSearch']> | undefined;
    const timer = setTimeout(() => {
      if (category === 'friends') {
        observer = owner.watchSearch(value);
        search = observer;
      } else {
        void owner.loadRequests(category, controller.signal).finally(() => {
          if (!controller.signal.aborted) searchingRequests = false;
        });
      }
    }, 200);
    return () => {
      clearTimeout(timer);
      controller.abort();
      untrack(() => observer?.stop());
    };
  });

  onMount(() => {
    list.scrollTop = contacts.view.scrollTop;
  });
  onDestroy(() => lifetime.abort());

  function selectCategory(category: ContactCategory) {
    void goto(`/app/contacts?category=${category}`, {
      reset: false,
    });
  }

  async function selectContact(id: string) {
    await goto(`/app/contacts?category=${tab}&person=${id}`, {
      reset: false,
    });
    await tick();
    detailPanel
      ?.querySelector<HTMLElement>('h2')
      ?.focus({ preventScroll: true });
  }

  async function backToList() {
    const previousId = selectedId;
    await goto(`/app/contacts?category=${tab}`, {
      reset: false,
    });
    await tick();
    list
      .querySelector<HTMLButtonElement>(`[data-contact-id="${previousId}"]`)
      ?.focus();
  }

  async function openChat(user: User) {
    const sync = chat.sync;
    if (!sync?.alive || busy) return;
    busy = true;
    error = '';
    try {
      const conversation = await startDirectConversation(
        user.id,
        undefined,
        lifetime.signal,
      );
      // A late response belongs to the initiating route and authenticated owner.
      if (lifetime.signal.aborted || !sync.alive || chat.sync !== sync) return;
      await sync.refreshSummaries();
      if (lifetime.signal.aborted || !sync.alive || chat.sync !== sync) return;
      await goto(`/app/chats/${conversation.id}`);
    } catch (reason) {
      if (!lifetime.signal.aborted)
        error = reason instanceof Error ? reason.message : '打开对话失败';
    } finally {
      if (!lifetime.signal.aborted) busy = false;
    }
  }
</script>

<svelte:head><title>通讯录 · Chat</title></svelte:head>

{#snippet contactRow(user: User, id: string, description?: string)}
  <li>
    <Button
      variant="ghost"
      class="contact-entry"
      aria-label={`${user.display_name} (@${user.username})${description ? `，${description}` : ''}`}
      aria-pressed={selectedId === id}
      data-contact-id={id}
      onclick={() => selectContact(id)}
    >
      <Avatar.Root data-tone={avatarTone(user.id)}>
        <Avatar.Fallback>
          {Array.from(user.display_name)[0] ?? '…'}
        </Avatar.Fallback>
      </Avatar.Root>
      <span class="contact-copy">
        <strong>{user.display_name}</strong>
        <small>{description ?? `@${user.username}`}</small>
      </span>
    </Button>
  </li>
{/snippet}

<main class="contacts-shell workspace-columns" class:has-contact={!!selectedId}>
  <aside class="contacts-sidebar" aria-label="通讯录列表">
    <PaneHeader title="通讯录" level={1}>
      {#snippet actions()}<Button
          variant="ghost"
          size="icon"
          class="rounded-full size-10"
          aria-label="添加好友"
          title="添加好友"
          onclick={() => (addOpen = true)}
        >
          <UserPlus class="size-5" />
        </Button><AccountMenu mobile />{/snippet}
    </PaneHeader>
    <label class="workspace-search">
      <Search size={17} aria-hidden="true" />
      <Input
        aria-label="搜索通讯录"
        class="h-11 bg-background pl-10 text-base md:text-base dark:bg-background"
        maxlength={64}
        placeholder={tab === 'friends' ? '搜索好友' : '搜索申请'}
        bind:value={query}
        oncompositionstart={() => (composing = true)}
        oncompositionend={() => (composing = false)}
      />
    </label>
    <nav class="contact-tabs" aria-label="通讯录分类">
      <Button
        variant={tab === 'friends' ? 'secondary' : 'ghost'}
        aria-pressed={tab === 'friends'}
        onclick={() => selectCategory('friends')}
      >
        好友
      </Button>
      <Button
        variant={tab !== 'friends' ? 'secondary' : 'ghost'}
        aria-pressed={tab !== 'friends'}
        onclick={() =>
          selectCategory(tab === 'outgoing' ? 'outgoing' : 'incoming')}
      >
        {tab === 'outgoing'
          ? '已发送'
          : '新的朋友'}{#if tab !== 'outgoing' && contacts.owner?.pendingCount}（{contacts
            .owner.pendingCount}）{/if}
      </Button>
      <DropdownMenu.Root>
        <DropdownMenu.Trigger aria-label="申请筛选" title="申请筛选">
          {#snippet child({ props })}<Button
              {...props}
              variant="ghost"
              size="icon"
            >
              <ChevronDown size={15} />
            </Button>{/snippet}
        </DropdownMenu.Trigger>
        <DropdownMenu.Content align="end">
          <DropdownMenu.Item onSelect={() => selectCategory('incoming')}>
            收到的申请
          </DropdownMenu.Item>
          <DropdownMenu.Item onSelect={() => selectCategory('outgoing')}>
            发出的申请
          </DropdownMenu.Item>
        </DropdownMenu.Content>
      </DropdownMenu.Root>
    </nav>
    {#if listError}
      <div class="contacts-feedback">
        <p class="form-error" role="alert">{listError}</p>
        <Button variant="secondary" onclick={() => contacts.owner?.refresh()}>
          重试
        </Button>
      </div>
    {/if}
    {#if contacts.owner?.loading || searching}<p
        class="contacts-feedback muted"
        role="status"
      >
        正在更新…
      </p>{/if}
    <div
      class="contacts-list-scroll"
      bind:this={list}
      onscroll={() => (contacts.view.scrollTop = list.scrollTop)}
    >
      <ul
        class="contact-list"
        aria-label={tabs.find((item) => item.id === tab)?.label}
      >
        {#if tab === 'friends'}
          {#each friends as friend (friend.user.id)}
            {@render contactRow(friend.user, friend.user.id)}
          {:else}
            {#if !contacts.owner?.loading && !searching}<li
                class="contact-empty"
              >
                {term ? '没有找到匹配的好友' : '还没有好友，先添加一位吧。'}
              </li>{/if}
          {/each}
        {:else}
          {#each filteredRequests as request (request.id)}
            {@render contactRow(
              tab === 'incoming' ? request.sender : request.recipient,
              request.id,
              tab === 'incoming' ? '请求添加你为好友' : '等待对方同意',
            )}
          {:else}
            {#if !contacts.owner?.loading && !searching}<li
                class="contact-empty"
              >
                {term ? '没有找到匹配的申请' : '暂无好友申请'}
              </li>{/if}
          {/each}
        {/if}
      </ul>
      {#if hasMore}
        <Button
          variant="secondary"
          class="contacts-load-more"
          disabled={contacts.owner?.loading || searching}
          onclick={() =>
            tab === 'friends' && term
              ? search?.loadMore()
              : contacts.owner?.loadMore(tab)}
        >
          加载更多
        </Button>
      {/if}
    </div>
  </aside>
  <section
    class="contact-detail-panel"
    bind:this={detailPanel}
    aria-label={tab === 'friends' ? '好友资料' : '好友申请'}
  >
    {#if selectedId}
      <PaneHeader title={tab === 'friends' ? '好友资料' : '好友申请'}>
        {#snippet leading()}<Button
            variant="ghost"
            size="icon"
            class="contacts-back rounded-full size-10"
            aria-label="返回通讯录列表"
            onclick={backToList}
          >
            <ArrowLeft class="size-5" />
          </Button>{/snippet}
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
            {#if selectedRequest}<div>
                <dt>申请时间</dt>
                <dd>{new Date(selectedRequest.created_at).toLocaleString()}</dd>
              </div>{/if}
          </dl>
          {#if tab !== 'friends'}
            <p class="contact-request-status">
              {tab === 'incoming'
                ? '对方请求添加你为好友'
                : '好友申请已发送，等待对方同意'}
            </p>
          {/if}
          {#if error || contacts.owner?.error}<p
              class="form-error"
              role="alert"
            >
              {error || contacts.owner?.error}
            </p>{/if}
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
                onclick={() => (removing = selectedUser)}
              >
                删除好友
              </Button>
            {:else if selectedRequest}
              {#if tab === 'incoming'}
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
          <Button variant="secondary" onclick={() => (addOpen = true)}>
            <UserPlus size={17} />添加好友
          </Button>
        {/if}
      </div>
    {/if}
  </section>
</main>
<AddFriendDialog bind:open={addOpen} />
<Dialog.Root
  open={!!removing}
  onOpenChange={(open) => {
    if (!open) removing = undefined;
  }}
>
  <Dialog.Content closeLabel="关闭">
    <Dialog.Title>删除好友</Dialog.Title>
    <Dialog.Description>
      删除 {removing?.display_name} 后将无法继续发送消息，已有聊天记录仍会保留。
    </Dialog.Description>
    <Button
      variant="destructive"
      disabled={contacts.owner?.busy}
      onclick={async () => {
        if (removing && (await contacts.owner?.remove(removing.id)))
          removing = undefined;
      }}
    >
      确认删除
    </Button>
    {#if contacts.owner?.mutationError}<p role="alert" class="form-error">
        {contacts.owner.mutationError}
      </p>{/if}
  </Dialog.Content>
</Dialog.Root>
