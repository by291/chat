import { untrack } from 'svelte';

import {
  InfiniteQueryObserver,
  QueryObserver,
  type InfiniteData,
  type InfiniteQueryObserverResult,
  type QueryClient,
  type QueryObserverResult,
} from '@tanstack/svelte-query';

import {
  createContactsApi,
  type FriendsPage,
  type RequestsPage,
  type Relationship,
  type RequestAction,
} from './api';

class ContactPages<Page extends { next_cursor?: string | null }> {
  result = $state.raw<InfiniteQueryObserverResult<InfiniteData<Page>>>();
  private query: InfiniteQueryObserver<Page>;
  private unsubscribe: () => void;

  constructor(
    cache: QueryClient,
    key: readonly string[],
    read: (cursor: string | undefined, signal: AbortSignal) => Promise<Page>,
    enabled = false,
  ) {
    this.query = new InfiniteQueryObserver<Page>(cache, {
      queryKey: key,
      queryFn: ({ pageParam, signal }) =>
        read(pageParam as string | undefined, signal),
      initialPageParam: undefined,
      getNextPageParam: (page) => page.next_cursor ?? undefined,
      enabled,
      retry: false,
      networkMode: 'always',
    });
    this.result = this.query.getCurrentResult();
    this.unsubscribe = this.query.subscribe((result) => (this.result = result));
  }

  load() {
    return this.query.refetch();
  }
  more() {
    return this.query.fetchNextPage({ cancelRefetch: false });
  }
  stop() {
    this.unsubscribe();
    this.query.destroy();
    this.result = undefined;
  }
}

/** Canonical contacts belong to the shared query cache for one authenticated lifetime. */
export class ContactsState {
  private active = true;
  private lifetime = new AbortController();
  private api: ReturnType<typeof createContactsApi>;
  private prefix: readonly string[];
  private friendsPages: ContactPages<FriendsPage>;
  private incomingPages: ContactPages<RequestsPage>;
  private outgoingPages: ContactPages<RequestsPage>;
  private relationshipCleanups = new Set<() => void>();
  private searchCleanups = new Set<() => void>();
  busy = $state(false);
  mutationError = $state('');

  constructor(
    private options: {
      queryClient: QueryClient;
      userId: string;
      api?: ReturnType<typeof createContactsApi>;
    },
  ) {
    this.api = options.api ?? createContactsApi();
    this.prefix = ['contacts', options.userId];
    this.friendsPages = new ContactPages(
      options.queryClient,
      [...this.prefix, 'friends'],
      (cursor, signal) => this.api.friends('', cursor, signal),
    );
    this.incomingPages = new ContactPages(
      options.queryClient,
      [...this.prefix, 'incoming'],
      (cursor, signal) => this.api.requests('incoming', cursor, signal),
    );
    this.outgoingPages = new ContactPages(
      options.queryClient,
      [...this.prefix, 'outgoing'],
      (cursor, signal) => this.api.requests('outgoing', cursor, signal),
    );
  }

  get friends() {
    return (
      this.friendsPages.result?.data?.pages.flatMap((page) => page.items) ?? []
    );
  }
  get incoming() {
    return (
      this.incomingPages.result?.data?.pages.flatMap((page) => page.items) ?? []
    );
  }
  get outgoing() {
    return (
      this.outgoingPages.result?.data?.pages.flatMap((page) => page.items) ?? []
    );
  }
  get pendingCount() {
    return (
      this.incomingPages.result?.data?.pages[0]?.pending_incoming_count ?? 0
    );
  }
  get loading() {
    return (
      this.friendsPages.result?.isFetching ||
      this.incomingPages.result?.isFetching ||
      this.outgoingPages.result?.isFetching
    );
  }
  get error() {
    const reason =
      this.friendsPages.result?.error ??
      this.incomingPages.result?.error ??
      this.outgoingPages.result?.error;
    return (
      this.mutationError || (reason instanceof Error ? reason.message : '')
    );
  }
  hasMore(kind: 'friends' | 'incoming' | 'outgoing') {
    return this.pages(kind).result?.hasNextPage ?? false;
  }
  loadMore(kind: 'friends' | 'incoming' | 'outgoing') {
    if (this.active) return this.pages(kind).more();
  }

  /** Reveal a deep link in canonical pages; route cancellation never cancels shared reads. */
  async reveal(
    kind: 'friends' | 'incoming' | 'outgoing',
    id: string,
    signal: AbortSignal,
  ) {
    await this.readUntil(kind, signal, () =>
      kind === 'friends'
        ? this.friends.some((friend) => friend.user.id === id)
        : this[kind].some((request) => request.id === id),
    );
  }

  /** Both request directions share one destination and stop when either contains the selection. */
  async revealRequest(id: string, signal: AbortSignal) {
    const found = () =>
      this.incoming.some((request) => request.id === id) ||
      this.outgoing.some((request) => request.id === id);
    await Promise.all([
      this.readUntil('incoming', signal, found),
      this.readUntil('outgoing', signal, found),
    ]);
  }

  /** Requests have no server search; finish pagination before declaring no matches. */
  async loadRequests(kind: 'incoming' | 'outgoing', signal: AbortSignal) {
    await this.readUntil(kind, signal, () => false);
  }

  private async readUntil(
    kind: 'friends' | 'incoming' | 'outgoing',
    signal: AbortSignal,
    found: () => boolean,
  ) {
    if (!this.active || signal.aborted) return;
    const pages = this.pages(kind);
    if (!pages.result?.data) await pages.more();

    while (this.active && !signal.aborted && !found()) {
      if (pages.result?.isError || !pages.result?.hasNextPage) return;
      const cursor = pages.result.data?.pages.at(-1)?.next_cursor;
      const count = pages.result.data?.pages.length ?? 0;
      await pages.more();
      // Refresh may cancel this read or make it await an existing refetch.
      // Only a newly appended page can prove the server repeated its cursor.
      if (
        (pages.result?.data?.pages.length ?? 0) > count &&
        pages.result?.data?.pages.at(-1)?.next_cursor === cursor
      )
        return;
    }
  }

  private pages(kind: 'friends' | 'incoming' | 'outgoing') {
    return kind === 'friends'
      ? this.friendsPages
      : kind === 'incoming'
        ? this.incomingPages
        : this.outgoingPages;
  }
  start() {
    return this.refresh();
  }

  async refresh() {
    if (!this.active) return;
    await this.options.queryClient.cancelQueries({ queryKey: this.prefix });
    if (!this.active) return;
    await Promise.all([
      this.friendsPages.load(),
      this.incomingPages.load(),
      this.outgoingPages.load(),
      this.options.queryClient.invalidateQueries({
        queryKey: [...this.prefix, 'relationship'],
      }),
      this.options.queryClient.invalidateQueries({
        queryKey: [...this.prefix, 'search'],
      }),
    ]);
  }

  /** Observer snapshots reference cache records; a caller owns only the subscription. */
  watchRelationship(
    userId: string,
    changed: (result: QueryObserverResult<Relationship>) => void,
  ) {
    // Query observers synchronously notify other cache owners. Their revision
    // counters must not become dependencies of the caller's identity effect.
    return untrack(() => {
      if (!this.active) return () => {};
      const query = new QueryObserver(this.options.queryClient, {
        queryKey: [...this.prefix, 'relationship', userId],
        queryFn: ({ signal }) => this.api.relationship(userId, signal),
        retry: false,
        networkMode: 'always',
      });
      changed(query.getCurrentResult());
      const unsubscribe = query.subscribe((result) => {
        if (this.active) changed(result);
      });
      const cleanup = () => {
        unsubscribe();
        query.destroy();
        this.relationshipCleanups.delete(cleanup);
      };
      this.relationshipCleanups.add(cleanup);
      return () => untrack(cleanup);
    });
  }

  /** Observe canonical search pages so contacts hints replace visible suggestions. */
  watchSearch(q: string) {
    if (!this.active)
      return {
        result: undefined,
        loadMore: async () => {},
        retry: async () => {},
        stop: () => {},
      };
    return untrack(() => {
      const pages = new ContactPages<FriendsPage>(
        this.options.queryClient,
        [...this.prefix, 'search', q],
        (cursor, signal) => this.api.friends(q, cursor, signal),
        this.active,
      );
      const cleanup = () => {
        pages.stop();
        this.searchCleanups.delete(cleanup);
      };
      this.searchCleanups.add(cleanup);
      return {
        get result() {
          return pages.result;
        },
        loadMore: () => pages.more(),
        retry: () => pages.load(),
        stop: () => untrack(cleanup),
      };
    });
  }

  request(id: string) {
    return this.mutate(() => this.api.request(id, this.lifetime.signal));
  }
  respond(id: string, action: RequestAction) {
    return this.mutate(() =>
      this.api.respond(id, action, this.lifetime.signal),
    );
  }
  remove(id: string) {
    return this.mutate(() => this.api.remove(id, this.lifetime.signal));
  }

  private async mutate(operation: () => Promise<unknown>) {
    if (!this.active || this.busy) return false;
    this.busy = true;
    this.mutationError = '';
    try {
      await operation();
      if (!this.active) return false;
      await this.refresh();
      return true;
    } catch (reason) {
      if (this.active)
        this.mutationError =
          reason instanceof Error ? reason.message : '操作失败，请重试';
      return false;
    } finally {
      if (this.active) this.busy = false;
    }
  }

  stop() {
    this.active = false;
    this.lifetime.abort();
    for (const cleanup of [...this.relationshipCleanups]) untrack(cleanup);
    for (const cleanup of [...this.searchCleanups]) untrack(cleanup);
    this.friendsPages.stop();
    this.incomingPages.stop();
    this.outgoingPages.stop();
    void this.options.queryClient.cancelQueries({ queryKey: this.prefix });
    this.options.queryClient.removeQueries({ queryKey: this.prefix });
  }
}
