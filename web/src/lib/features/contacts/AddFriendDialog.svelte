<script lang="ts">
  import { untrack } from 'svelte';
  import type { QueryObserverResult } from '@tanstack/svelte-query';

  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Label } from '#lib/components/ui/label/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';

  import { createContactsApi, type User, type Relationship } from './api';
  import { useContacts } from './context.svelte';

  let {
    open = $bindable(false),
    user,
    onCloseAutoFocus,
  }: {
    open: boolean;
    user?: User;
    onCloseAutoFocus?: (event: Event) => void;
  } = $props();
  const contacts = useContacts();
  const api = createContactsApi();
  let query = $state('');
  let selected = $state<User>();
  let results = $state<User[]>([]);
  let searching = $state(false);
  let composing = $state(false);
  let error = $state('');
  let relationship = $state.raw<QueryObserverResult<Relationship>>();
  const pending = $derived(relationship?.data?.pending_request);
  const incoming = $derived(pending?.sender.id === selected?.id);

  $effect(() => {
    if (open) {
      untrack(() => {
        selected = user;
        query = '';
        error = '';
      });
    }
  });

  $effect(() => {
    const owner = contacts.owner;
    const id = selected?.id;
    relationship = undefined;
    if (open && owner && id) {
      return owner.watchRelationship(id, (value) => (relationship = value));
    }
  });

  $effect(() => {
    const term = query.trim();
    results = [];
    error = '';
    const enabled = open && !!term && !composing;
    searching = enabled;
    if (!enabled) return;
    const controller = new AbortController();
    const timer = setTimeout(async () => {
      try {
        const users = await api.searchUsers(term, controller.signal);
        if (!controller.signal.aborted) results = users;
      } catch (reason) {
        if (!controller.signal.aborted) {
          error = reason instanceof Error ? reason.message : '搜索失败';
        }
      } finally {
        if (!controller.signal.aborted) searching = false;
      }
    }, 250);
    return () => {
      clearTimeout(timer);
      controller.abort();
    };
  });
</script>

<Dialog.Root bind:open>
  <Dialog.Content closeLabel="关闭" {onCloseAutoFocus}>
    <Dialog.Header>
      <Dialog.Title>添加好友</Dialog.Title>
      <Dialog.Description>
        通过用户名或昵称找到对方，同意申请后即可聊天。
      </Dialog.Description>
    </Dialog.Header>
    <Label for="friend-search">用户名或昵称</Label>
    <Input
      id="friend-search"
      bind:value={query}
      placeholder="搜索用户"
      maxlength={64}
      oncompositionstart={() => (composing = true)}
      oncompositionend={() => (composing = false)}
    />
    {#if searching}
      <p role="status">正在搜索…</p>
    {/if}
    {#if error}
      <p class="form-error" role="alert">{error}</p>
    {/if}
    <ul class="contact-list" aria-label="搜索结果">
      {#each results as result (result.id)}
        <li>
          <Button
            type="button"
            variant="ghost"
            class="w-full justify-start"
            onclick={() => (selected = result)}
          >
            {result.display_name}
            <span class="muted">@{result.username}</span>
          </Button>
        </li>
      {:else}
        {#if query.trim() && !searching && !error}
          <li class="muted">没有找到匹配的用户</li>
        {/if}
      {/each}
    </ul>
    {#if selected}
      <div class="contact-detail">
        <strong>{selected.display_name}</strong>
        <span class="muted">@{selected.username}</span>
        {#if relationship?.isError}
          <p class="form-error" role="alert">无法获取好友状态</p>
          <Button variant="secondary" onclick={() => contacts.owner?.refresh()}>
            重试
          </Button>
        {:else if !relationship?.data || relationship.isFetching}
          <p role="status">正在确认好友状态…</p>
        {:else if relationship.data.is_friend}
          <p role="status">已是好友</p>
        {:else if pending}
          {#if incoming}
            <p>对方已向你发送好友申请</p>
            <Button
              disabled={contacts.owner?.busy}
              onclick={() => contacts.owner?.respond(pending.id, 'accept')}
            >
              接受申请
            </Button>
          {:else}
            <p role="status">好友申请已发送，等待对方同意</p>
            <Button
              variant="secondary"
              disabled={contacts.owner?.busy}
              onclick={() => contacts.owner?.respond(pending.id, 'cancel')}
            >
              取消申请
            </Button>
          {/if}
        {:else}
          <Button
            disabled={contacts.owner?.busy}
            onclick={() => contacts.owner?.request(selected!.id)}
          >
            发送好友申请
          </Button>
        {/if}
      </div>
    {/if}
    {#if contacts.owner?.mutationError}
      <p class="form-error" role="alert">
        {contacts.owner.mutationError}
      </p>
    {/if}
  </Dialog.Content>
</Dialog.Root>
