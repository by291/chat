import { QueryClient } from '@tanstack/svelte-query';
import { expect, test, vi } from 'vitest';

import { ContactsState } from '../../src/lib/features/contacts/contacts.svelte';
import { createContactsApi } from '../../src/lib/features/contacts/api';

const friend = {
  user: { id: 'bob', username: 'bob', display_name: 'Bob' },
  created_at: '2026-10-20T00:00:00Z',
};

test('contacts refresh replaces canonical permissions and stop fences late results', async () => {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  let items = [friend];
  let resolve!: (value: { items: typeof items; next_cursor: null }) => void;
  let delayed = false;
  const owner = new ContactsState({
    queryClient: cache,
    userId: 'alice',
    api: {
      ...createContactsApi(),
      friends: async () =>
        delayed
          ? new Promise((done) => {
              resolve = done;
            })
          : { items, next_cursor: null },
      requests: async () => ({
        items: [],
        next_cursor: null,
        pending_incoming_count: 2,
      }),
    },
  });
  await owner.start();
  expect(owner.friends).toEqual([friend]);
  expect(owner.pendingCount).toBe(2);
  items = [];
  await owner.refresh();
  expect(owner.friends).toEqual([]);

  delayed = true;
  const refreshing = owner.refresh();
  await vi.waitFor(() => expect(resolve).toBeTypeOf('function'));
  owner.stop();
  resolve({ items: [friend], next_cursor: null });
  await refreshing;

  expect(owner.friends).toEqual([]);
  expect(cache.getQueryCache().getAll()).toHaveLength(0);
});

test('a burst during a read coalesces into one fresh follow-up and relationship observers see mutations', async () => {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  let resolve!: (value: {
    items: (typeof friend)[];
    next_cursor: null;
  }) => void;
  let calls = 0;
  let isFriend = true;
  const owner = new ContactsState({
    queryClient: cache,
    userId: 'alice',
    api: {
      ...createContactsApi(),
      friends: async () => {
        calls++;
        return calls === 1
          ? new Promise((done) => (resolve = done))
          : { items: [], next_cursor: null };
      },
      requests: async () => ({
        items: [],
        next_cursor: null,
        pending_incoming_count: 0,
      }),
      relationship: async () => ({
        user: friend.user,
        is_friend: isFriend,
        pending_request: null,
      }),
      remove: async () => {
        isFriend = false;
        return undefined;
      },
    },
  });
  let relationship: boolean | undefined;
  const unsubscribe = owner.watchRelationship('bob', (result) => {
    relationship = result.data?.is_friend;
  });
  const initial = owner.start();
  await vi.waitFor(() => expect(calls).toBe(1));
  const refreshes = [owner.refresh(), owner.refresh(), owner.refresh()];
  resolve({ items: [friend], next_cursor: null });
  await Promise.all([initial, ...refreshes]);
  expect(calls).toBe(2);
  expect(owner.friends).toEqual([]);
  await vi.waitFor(() => expect(relationship).toBe(true));
  await owner.remove('bob');
  expect(relationship).toBe(false);
  unsubscribe();
  owner.stop();
});

test('observed friend searches recover from refresh and stop showing a removed friend', async () => {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  let finish!: (value: { items: (typeof friend)[]; next_cursor: null }) => void;
  let calls = 0;
  let removed = false;
  const owner = new ContactsState({
    queryClient: cache,
    userId: 'alice',
    api: {
      ...createContactsApi(),
      friends: async (q) => {
        if (!q) return { items: [], next_cursor: null };
        calls++;
        if (calls === 1) return new Promise((resolve) => (finish = resolve));
        return { items: removed ? [] : [friend], next_cursor: null };
      },
      requests: async () => ({
        items: [],
        next_cursor: null,
        pending_incoming_count: 0,
      }),
    },
  });
  const search = owner.watchSearch('bob');
  await vi.waitFor(() => expect(calls).toBe(1));
  await owner.refresh();
  expect(calls).toBe(2);
  expect(search.result?.data?.pages[0].items).toEqual([friend]);
  removed = true;
  await owner.refresh();
  finish({ items: [friend], next_cursor: null });
  await Promise.resolve();
  expect(search.result?.data?.pages[0].items).toEqual([]);
  expect(search.result?.error).toBeNull();
  search.stop();
  owner.stop();
});

test('reconnect refresh fences delayed relationship and badge reads', async () => {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  let oldRelationship!: (value: {
    user: typeof friend.user;
    is_friend: boolean;
    pending_request: null;
  }) => void;
  let oldIncoming!: (value: {
    items: [];
    next_cursor: null;
    pending_incoming_count: number;
  }) => void;
  let relationships = 0;
  let incoming = 0;
  const owner = new ContactsState({
    queryClient: cache,
    userId: 'alice',
    api: {
      ...createContactsApi(),
      friends: async () => ({ items: [], next_cursor: null }),
      requests: async (direction) =>
        direction === 'incoming' && ++incoming === 1
          ? new Promise((resolve) => (oldIncoming = resolve))
          : { items: [], next_cursor: null, pending_incoming_count: 0 },
      relationship: async () =>
        ++relationships === 1
          ? new Promise((resolve) => (oldRelationship = resolve))
          : { user: friend.user, is_friend: false, pending_request: null },
    },
  });
  let permission: boolean | undefined;
  const cleanup = owner.watchRelationship(
    'bob',
    (result) => (permission = result.data?.is_friend),
  );
  const start = owner.start();
  await vi.waitFor(() => expect(oldIncoming).toBeTypeOf('function'));
  await owner.refresh();
  oldRelationship({
    user: friend.user,
    is_friend: true,
    pending_request: null,
  });
  oldIncoming({ items: [], next_cursor: null, pending_incoming_count: 4 });
  await start;
  await Promise.resolve();
  expect(permission).toBe(false);
  expect(owner.pendingCount).toBe(0);
  cleanup();
  owner.stop();
  const disposed = owner.watchSearch('bob');
  expect(disposed.result).toBeUndefined();
  expect(cache.getQueryCache().getAll()).toHaveLength(0);
});

test('contact deep links load canonical pages until the selected record is available', async () => {
  const cache = new QueryClient();
  const friends = vi.fn(async (_query = '', cursor?: string) => ({
    items: cursor ? [friend] : [],
    next_cursor: cursor ? null : 'next-page',
  }));
  const owner = new ContactsState({
    queryClient: cache,
    userId: 'alice',
    api: {
      ...createContactsApi(),
      friends,
      requests: async () => ({
        items: [],
        next_cursor: null,
        pending_incoming_count: 0,
      }),
    },
  });
  await owner.start();
  expect(owner.friends).toEqual([]);

  await owner.reveal('friends', 'bob', new AbortController().signal);

  expect(owner.friends).toEqual([friend]);
  expect(friends.mock.calls.some(([, cursor]) => cursor === 'next-page')).toBe(
    true,
  );
  owner.stop();
});

test('request search loads later pages and a cancelled route does not start more reads', async () => {
  const cache = new QueryClient();
  const request = {
    id: 'request-bob',
    sender: friend.user,
    recipient: { id: 'alice', username: 'alice', display_name: 'Alice' },
    status: 'pending' as const,
    created_at: friend.created_at,
    updated_at: friend.created_at,
  };
  const requests = vi.fn(
    async (_direction: 'incoming' | 'outgoing', cursor?: string) => ({
      items: cursor ? [request] : [],
      next_cursor: cursor ? null : 'next-page',
      pending_incoming_count: 1,
    }),
  );
  const owner = new ContactsState({
    queryClient: cache,
    userId: 'alice',
    api: {
      ...createContactsApi(),
      friends: async () => ({ items: [], next_cursor: null }),
      requests,
    },
  });
  await owner.start();
  const route = new AbortController();
  route.abort();

  await owner.loadRequests('incoming', route.signal);
  expect(requests).toHaveBeenCalledTimes(2);
  await owner.loadRequests('incoming', new AbortController().signal);

  expect(owner.incoming).toEqual([request]);
  expect(owner.hasMore('incoming')).toBe(false);
  expect(requests).toHaveBeenCalledTimes(3);
  owner.stop();
});

test('a refresh during deep-link pagination resumes the interrupted next page', async () => {
  const cache = new QueryClient();
  let release!: (page: { items: (typeof friend)[]; next_cursor: null }) => void;
  let nextReads = 0;
  const owner = new ContactsState({
    queryClient: cache,
    userId: 'alice',
    api: {
      ...createContactsApi(),
      friends: async (_query, cursor) => {
        if (!cursor) return { items: [], next_cursor: 'page-two' };
        nextReads++;
        if (nextReads === 1)
          return new Promise((resolve) => {
            release = resolve;
          });
        return { items: [friend], next_cursor: null };
      },
      requests: async () => ({
        items: [],
        next_cursor: null,
        pending_incoming_count: 0,
      }),
    },
  });
  await owner.start();
  const revealing = owner.reveal(
    'friends',
    'bob',
    new AbortController().signal,
  );
  await vi.waitFor(() => expect(nextReads).toBe(1));

  await owner.refresh();
  await revealing;

  expect(owner.friends).toEqual([friend]);
  release({ items: [friend], next_cursor: null });
  owner.stop();
});
