import { afterEach, expect, test } from 'vitest';

import { createQueryClient, keys } from '../../src/lib/features/chat/cache';
import { ConversationStore } from '../../src/lib/features/chat/synchronization/conversation-store';
import type { ChatApi } from '../../src/lib/features/chat/models';

const cleanups: Array<() => void> = [];

afterEach(() => {
  for (const cleanup of cleanups.splice(0)) cleanup();
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });

  return { promise, resolve, reject };
}

function fixture(overrides: Partial<ChatApi> = {}) {
  const unexpectedRequest = async (): Promise<never> => {
    throw new Error('Unexpected API operation');
  };
  const api: ChatApi = {
    list: async () => ({ items: [], next_cursor: null }),
    snapshot: async () => ({ items: [], snapshot_seq: '0', next_before: null }),
    events: unexpectedRequest,
    conversation: unexpectedRequest,
    send: unexpectedRequest,
    edit: unexpectedRequest,
    withdraw: unexpectedRequest,
    setReaction: unexpectedRequest,
    removeReaction: unexpectedRequest,
    search: unexpectedRequest,
    context: unexpectedRequest,
    preferences: unexpectedRequest,
    read: unexpectedRequest,
    typing: unexpectedRequest,
    status: unexpectedRequest,
    members: unexpectedRequest,
    ...overrides,
  };
  const queryClient = createQueryClient();
  const failures: string[] = [];
  let active = true;
  const store = new ConversationStore({
    api,
    queryClient,
    userId: 'alice',
    active: () => active,
    epoch: () => 0,
    selectedId: () => 'chat',
    changed: () => {},
    failed: (message) => failures.push(message),
    messagesMerged: () => {},
    peerSelected: () => {},
    membershipChanged: () => {},
  });
  cleanups.push(() => {
    active = false;
    store.stop();
    queryClient.clear();
  });

  return { store, queryClient, failures };
}

test('concurrent summary callers share one handled failure and a later refresh retries', async () => {
  const response = deferred<Awaited<ReturnType<ChatApi['list']>>>();
  let requests = 0;
  const { store, failures } = fixture({
    list: () => {
      requests++;
      return requests === 1
        ? response.promise
        : Promise.resolve({ items: [], next_cursor: null });
    },
  });

  const outcomes = Promise.allSettled([
    store.refreshSummaries(),
    store.refreshSummaries(),
  ]);
  response.reject(new Error('summary unavailable'));

  expect((await outcomes).map((outcome) => outcome.status)).toEqual([
    'fulfilled',
    'fulfilled',
  ]);
  expect(failures).toEqual(['summary unavailable']);
  expect(requests).toBe(1);

  await store.refreshSummaries();

  expect(requests).toBe(2);
  expect(failures).toEqual(['summary unavailable']);
});

test('concurrent replay callers share one handled failure and retry the unapplied cursor', async () => {
  const response = deferred<Awaited<ReturnType<ChatApi['events']>>>();
  const cursors: string[] = [];
  const { store, failures } = fixture({
    events: (_id, after) => {
      cursors.push(after);
      return cursors.length === 1
        ? response.promise
        : Promise.resolve({
            events: [
              {
                envelope: {
                  schema_version: 1,
                  event_id: 'receipt',
                  kind: 'receipt.updated',
                  conversation_id: 'chat',
                  event_seq: '1',
                },
                payload: {
                  type: 'receipt.updated',
                  data: { user_id: 'alice', message_seq: '0' },
                },
              },
            ],
            last_seq: '1',
            snapshot_seq: '1',
            has_more: false,
          });
    },
  });
  await store.select('chat', async () => {});
  store.raiseTarget('chat', '1');

  const outcomes = Promise.allSettled([
    store.drain('chat'),
    store.drain('chat'),
  ]);
  response.reject(new Error('replay unavailable'));

  expect((await outcomes).map((outcome) => outcome.status)).toEqual([
    'fulfilled',
    'fulfilled',
  ]);
  expect(failures).toEqual(['replay unavailable']);
  expect(cursors).toEqual(['0']);

  await store.drain('chat');

  expect(cursors).toEqual(['0', '0']);
  expect(failures).toEqual(['replay unavailable']);
});

test('chat teardown removes chat records without evicting another feature cache', () => {
  const { store, queryClient } = fixture();
  queryClient.setQueryData(keys.conversations, []);
  queryClient.setQueryData(keys.messages('chat'), []);
  queryClient.setQueryData(keys.members('chat'), []);
  queryClient.setQueryData(['contacts', 'alice'], ['bob']);

  store.stop();

  expect(queryClient.getQueryData(keys.conversations)).toBeUndefined();
  expect(queryClient.getQueryData(keys.messages('chat'))).toBeUndefined();
  expect(queryClient.getQueryData(keys.members('chat'))).toBeUndefined();
  expect(queryClient.getQueryData(['contacts', 'alice'])).toEqual(['bob']);
});
