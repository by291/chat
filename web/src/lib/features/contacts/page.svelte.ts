import { onDestroy, untrack } from 'svelte';
import { goto } from '$app/navigation';
import { page } from '$app/state';

import { startDirectConversation } from '#lib/features/chat/api.ts';
import { useChat } from '#lib/features/chat/context.svelte.ts';

import type { User } from './api';
import type { ContactsState } from './contacts.svelte';
import { useContacts, type ContactCategory } from './context.svelte';
import { mergeRequests } from './requests/model';

const SEARCH_DEBOUNCE_MS = 200;

/**
 * Route-owned search and pagination over the authenticated contacts cache.
 * The URL remains canonical; only requests and transient input live here.
 */
export function createContactsPage(resetScroll: () => void) {
  const contacts = useContacts();
  const chat = useChat();
  const lifetime = new AbortController();

  let busy = $state(false);
  let error = $state('');
  let query = $state('');
  let composing = $state(false);
  let revealing = $state(false);
  let searchingRequests = $state(false);
  let search = $state.raw<ReturnType<ContactsState['watchSearch']>>();

  // Preserve old request deep links while presenting one combined category.
  const tab: ContactCategory = $derived(
    ['requests', 'incoming', 'outgoing'].includes(
      page.url.searchParams.get('category') ?? '',
    )
      ? 'requests'
      : 'friends',
  );
  const selectedId = $derived(page.url.searchParams.get('person'));

  const requests = $derived(
    mergeRequests(
      contacts.owner?.incoming ?? [],
      contacts.owner?.outgoing ?? [],
    ),
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
      : selectedRequest?.user,
  );

  const term = $derived(query.trim());
  const friends = $derived(
    term
      ? (search?.result?.data?.pages.flatMap((page) => page.items) ?? [])
      : (contacts.owner?.friends ?? []),
  );
  const filteredRequests = $derived(
    requests.filter((request) => {
      const user = request.user;
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
      : tab === 'friends'
        ? contacts.owner?.hasMore('friends')
        : contacts.owner?.hasMore('incoming') ||
          contacts.owner?.hasMore('outgoing'),
  );

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
      if (!lifetime.signal.aborted) {
        error = reason instanceof Error ? reason.message : '打开对话失败';
      }
    } finally {
      if (!lifetime.signal.aborted) busy = false;
    }
  }

  function loadMore() {
    if (tab === 'friends') {
      return term ? search?.loadMore() : contacts.owner?.loadMore('friends');
    }

    return Promise.all(
      (['incoming', 'outgoing'] as const)
        .filter((direction) => contacts.owner?.hasMore(direction))
        .map((direction) => contacts.owner?.loadMore(direction)),
    );
  }

  // The URL owns selection, including browser Back/Forward. Context only remembers
  // the last destination for the main navigation and the list's reading position.
  $effect(() => {
    const category = tab;
    const id = selectedId;
    untrack(() => {
      if (contacts.view.category !== category) {
        contacts.view.scrollTop = 0;
        resetScroll();
      }
      contacts.view.category = category;
      contacts.view.selectedId = id;
    });
  });

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
        void (
          category === 'friends'
            ? owner.reveal('friends', id, controller.signal)
            : owner.revealRequest(id, controller.signal)
        ).finally(() => {
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
        void Promise.all([
          owner.loadRequests('incoming', controller.signal),
          owner.loadRequests('outgoing', controller.signal),
        ]).finally(() => {
          if (!controller.signal.aborted) searchingRequests = false;
        });
      }
    }, SEARCH_DEBOUNCE_MS);

    return () => {
      clearTimeout(timer);
      controller.abort();
      untrack(() => observer?.stop());
    };
  });

  onDestroy(() => lifetime.abort());

  return {
    get tab() {
      return tab;
    },
    get selectedId() {
      return selectedId;
    },
    get selectedRequest() {
      return selectedRequest;
    },
    get selectedUser() {
      return selectedUser;
    },
    get term() {
      return term;
    },
    get friends() {
      return friends;
    },
    get filteredRequests() {
      return filteredRequests;
    },
    get searching() {
      return searching;
    },
    get listError() {
      return listError;
    },
    get hasMore() {
      return hasMore;
    },
    get revealing() {
      return revealing;
    },
    get busy() {
      return busy;
    },
    get error() {
      return error;
    },
    get query() {
      return query;
    },
    set query(value: string) {
      query = value;
    },
    get composing() {
      return composing;
    },
    set composing(value: boolean) {
      composing = value;
    },
    openChat,
    loadMore,
  };
}
