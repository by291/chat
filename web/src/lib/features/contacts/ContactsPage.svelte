<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { goto } from '$app/navigation';
  import { Search, UserPlus } from '@lucide/svelte';

  import { Input } from '#lib/components/ui/input/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import PaneHeader from '#lib/components/PaneHeader.svelte';
  import WorkspaceNavigation from '#lib/features/workspace/WorkspaceNavigation.svelte';

  import AddFriendDialog from './AddFriendDialog.svelte';
  import ContactDetails from './ContactDetails.svelte';
  import FriendsList from './friends/FriendsList.svelte';
  import RemoveFriendDialog from './friends/RemoveFriendDialog.svelte';
  import RequestsList from './requests/RequestsList.svelte';
  import { useContacts, type ContactCategory } from './context.svelte';
  import { createContactsPage } from './page.svelte';
  import type { User } from './api';
  import './contacts.css';

  const contacts = useContacts();
  let addOpen = $state(false);
  let removing = $state<User>();
  let list: HTMLElement;
  let detailPanel = $state<HTMLElement>();

  const controller = createContactsPage(() => {
    if (list) list.scrollTop = 0;
  });

  function selectCategory(category: ContactCategory) {
    void goto(`/app/contacts?category=${category}`, {
      reset: false,
    });
  }

  async function selectContact(id: string) {
    await goto(`/app/contacts?category=${controller.tab}&person=${id}`, {
      reset: false,
    });
    await tick();
    detailPanel
      ?.querySelector<HTMLElement>('h2')
      ?.focus({ preventScroll: true });
  }

  async function backToList() {
    const previousId = controller.selectedId;
    await goto(`/app/contacts?category=${controller.tab}`, {
      reset: false,
    });
    await tick();
    list
      .querySelector<HTMLButtonElement>(`[data-contact-id="${previousId}"]`)
      ?.focus();
  }
  onMount(() => {
    list.scrollTop = contacts.view.scrollTop;
  });
</script>

<svelte:head><title>通讯录 · Chat</title></svelte:head>

<main
  class="contacts-shell workspace-columns"
  class:has-contact={!!controller.selectedId}
>
  <aside class="contacts-sidebar" aria-label="通讯录列表">
    <WorkspaceNavigation />
    <PaneHeader title="通讯录" level={1}>
      {#snippet actions()}
        <Button
          variant="ghost"
          size="icon"
          class="rounded-full size-10"
          aria-label="添加好友"
          title="添加好友"
          onclick={() => (addOpen = true)}
        >
          <UserPlus class="size-5" />
        </Button>
      {/snippet}
    </PaneHeader>
    <label class="workspace-search">
      <Search size={17} aria-hidden="true" />
      <Input
        aria-label="搜索通讯录"
        class="h-11 bg-background pl-10 text-base md:text-base dark:bg-background"
        maxlength={64}
        placeholder={controller.tab === 'friends' ? '搜索好友' : '搜索申请'}
        bind:value={controller.query}
        oncompositionstart={() => (controller.composing = true)}
        oncompositionend={() => (controller.composing = false)}
      />
    </label>
    <nav class="contact-tabs" aria-label="通讯录分类">
      <Button
        variant={controller.tab === 'friends' ? 'secondary' : 'ghost'}
        aria-pressed={controller.tab === 'friends'}
        onclick={() => selectCategory('friends')}
      >
        好友
      </Button>
      <Button
        variant={controller.tab === 'requests' ? 'secondary' : 'ghost'}
        aria-pressed={controller.tab === 'requests'}
        onclick={() => selectCategory('requests')}
      >
        新的朋友{#if contacts.owner?.pendingCount}（{contacts.owner
            .pendingCount}）{/if}
      </Button>
    </nav>
    {#if controller.listError}
      <div class="contacts-feedback">
        <p class="form-error" role="alert">{controller.listError}</p>
        <Button variant="secondary" onclick={() => contacts.owner?.refresh()}>
          重试
        </Button>
      </div>
    {/if}
    {#if contacts.owner?.loading || controller.searching}
      <p class="contacts-feedback muted" role="status">正在更新…</p>
    {/if}
    <div
      class="contacts-list-scroll"
      bind:this={list}
      onscroll={() => (contacts.view.scrollTop = list.scrollTop)}
    >
      <ul
        class="contact-list"
        aria-label={controller.tab === 'friends' ? '好友' : '新的朋友'}
      >
        {#if controller.tab === 'friends'}
          <FriendsList
            friends={controller.friends}
            selectedId={controller.selectedId}
            loading={!!contacts.owner?.loading}
            searching={!!controller.searching}
            term={controller.term}
            onselect={selectContact}
          />
        {:else}
          <RequestsList
            requests={controller.filteredRequests}
            selectedId={controller.selectedId}
            loading={!!contacts.owner?.loading}
            searching={!!controller.searching}
            term={controller.term}
            onselect={selectContact}
          />
        {/if}
      </ul>
      {#if controller.hasMore}
        <Button
          variant="secondary"
          class="contacts-load-more"
          disabled={contacts.owner?.loading || controller.searching}
          onclick={controller.loadMore}
        >
          加载更多
        </Button>
      {/if}
    </div>
  </aside>
  <section
    class="contact-detail-panel"
    bind:this={detailPanel}
    aria-label={controller.tab === 'friends' ? '好友资料' : '好友申请'}
  >
    <ContactDetails
      tab={controller.tab}
      selectedId={controller.selectedId}
      selectedUser={controller.selectedUser}
      selectedRequest={controller.selectedRequest}
      revealing={controller.revealing}
      busy={controller.busy}
      error={controller.error}
      openChat={controller.openChat}
      onback={backToList}
      onremove={(user) => (removing = user)}
      onadd={() => (addOpen = true)}
    />
  </section>
</main>
<AddFriendDialog bind:open={addOpen} />
<RemoveFriendDialog bind:removing />
