import { afterEach, expect, test } from 'vitest';

import type { components } from '../../src/lib/api-client';
import { createQueryClient, keys } from '../../src/lib/features/chat/cache';
import { GroupRecords } from '../../src/lib/features/chat/group-records';
import {
  createChatSync,
  type ChatSync,
} from '../../src/lib/features/chat/sync.svelte';
import type {
  ChatApi,
  GroupMember,
  Message,
  RealtimeEvent,
  Snapshot,
} from '../../src/lib/features/chat/models';
import type { RealtimeTransport } from '../../src/lib/features/chat/realtime';

type Group = components['schemas']['GroupConversationView'];
const running: ChatSync[] = [];

afterEach(() => {
  for (const sync of running) sync.stop();
  running.length = 0;
});

function group(membership = 'old', sequence = '1'): Group {
  return {
    id: 'group',
    kind: 'group',
    title: 'Team',
    member_count: 2,
    membership: { membership_id: membership, joined_seq: sequence },
    my_role: 'member',
    latest_event_seq: sequence,
    last_message: null,
    unread_count: 0,
    my_read_seq: '0',
  };
}

function message(id: string, sequence = '1'): Message {
  return {
    id,
    conversation_id: 'group',
    sender_id: 'bob',
    client_message_id: id,
    body: id,
    created_seq: sequence,
    created_at: '2026-10-13T00:00:00Z',
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

class Transport implements RealtimeTransport {
  receive: (event: RealtimeEvent) => void = () => {};
  async subscribe(receive: (event: RealtimeEvent) => void) {
    this.receive = receive;
  }
  close() {}
}

function fixture(overrides: Partial<ChatApi> = {}) {
  const api: ChatApi = {
    list: async () => ({ items: [group()], next_cursor: null }),
    conversation: async () => group(),
    snapshot: async () => ({
      items: [message('old')],
      snapshot_seq: '1',
      next_before: null,
      membership: group().membership,
    }),
    events: async (_id, after) => ({
      events: [],
      last_seq: after,
      snapshot_seq: after,
      has_more: false,
      membership: group().membership,
    }),
    send: async () => {
      throw new Error('offline');
    },
    read: async (_id, sequence) => ({ my_read_seq: sequence }),
    typing: async () => {},
    status: async () => ({ online: false }),
    members: async () => [],
    ...overrides,
  };
  const transport = new Transport();
  const queryClient = createQueryClient();
  const sync = createChatSync({ api, transport, queryClient, userId: 'alice' });
  running.push(sync);
  return { sync, transport, queryClient, api };
}

function rosterMember(
  id: string,
  membership = id,
  readSequence = '0',
): GroupMember {
  return {
    user: { id, username: id, display_name: id },
    membership_id: membership,
    joined_seq: '1',
    read_seq: readSequence,
    role: id === 'alice' ? 'owner' : 'member',
  };
}

function rosterFixture(members: ChatApi['members']) {
  const { api, queryClient } = fixture({ members });
  const records = new GroupRecords({
    api,
    queryClient,
    userId: 'alice',
    membership: () => 'old',
    active: () => true,
    epoch: () => 1,
    failed: (reason) => {
      throw new Error(reason);
    },
  });
  return { records, queryClient };
}

test('a delayed roster preserves newer receipts only for the same member episode', async () => {
  const response = deferred<GroupMember[]>();
  const { records, queryClient } = rosterFixture(() => response.promise);
  queryClient.setQueryData(keys.members('group'), [
    rosterMember('alice', 'old'),
    rosterMember('bob', 'bob-old', '10'),
    rosterMember('carol', 'carol-old', '99'),
  ]);
  const loading = records.load('group');

  records.receipt('group', 'bob-old', '9007199254740994');
  response.resolve([
    rosterMember('alice', 'old'),
    rosterMember('bob', 'bob-old', '10'),
    rosterMember('carol', 'carol-new', '1'),
  ]);
  await loading;

  expect(records.members('group').map((member) => member.read_seq)).toEqual([
    '0',
    '9007199254740994',
    '1',
  ]);
});

test('roster hints during a request coalesce into a follow-up that removes ended members', async () => {
  const response = deferred<GroupMember[]>();
  let requests = 0;
  const { records } = rosterFixture(async () =>
    ++requests === 1 ? response.promise : [rosterMember('alice', 'old')],
  );
  const loading = records.load('group');

  const firstHint = records.load('group');
  const secondHint = records.load('group');
  response.resolve([rosterMember('alice', 'old'), rosterMember('bob')]);
  await Promise.all([loading, firstHint, secondHint]);

  expect(records.members('group').map((member) => member.user.id)).toEqual([
    'alice',
  ]);
  expect(requests).toBe(2);
});

test('forget fences a late roster even when the actor episode is unchanged', async () => {
  const response = deferred<GroupMember[]>();
  const { records } = rosterFixture(() => response.promise);
  const loading = records.load('group');

  records.forget('group');
  response.resolve([rosterMember('alice', 'old'), rosterMember('bob')]);
  await loading;

  expect(records.members('group')).toEqual([]);
});

test('an aborted roster does not publish or rerun queued hints', async () => {
  const response = deferred<GroupMember[]>();
  let requests = 0;
  const { records } = rosterFixture(() => {
    requests++;
    return response.promise;
  });
  const controller = new AbortController();
  const loading = records.load('group', controller.signal);
  const hint = records.load('group');

  controller.abort();
  response.resolve([rosterMember('alice', 'old')]);
  await Promise.all([loading, hint]);

  expect(records.members('group')).toEqual([]);
  expect(requests).toBe(1);
});

test('rejoining replaces the stream and discards an old snapshot response', async () => {
  let current = group();
  const old = deferred<Snapshot>();
  let snapshots = 0;
  const { sync } = fixture({
    list: async () => ({ items: [current], next_cursor: null }),
    snapshot: async () =>
      ++snapshots === 1
        ? old.promise
        : {
            items: [message('new', '10')],
            snapshot_seq: '10',
            next_before: null,
            membership: current.membership,
          },
  });
  await sync.start();
  const loading = sync.select('group');
  await expect.poll(() => snapshots).toBe(1);

  current = group('new', '10');
  await sync.refreshSummaries();
  old.resolve({
    items: [message('private-old')],
    snapshot_seq: '1',
    next_before: null,
    membership: group().membership,
  });
  await loading;
  await sync.select('group');

  expect(sync.messages.map((item) => item.id)).toEqual(['new']);
  expect(sync.appliedSeq).toBe('10');
});

test('a group deep link selected before initial summaries adopts the authorized episode', async () => {
  const summaries = deferred<{ items: Group[]; next_cursor: null }>();
  const { sync } = fixture({ list: () => summaries.promise });
  const starting = sync.start();
  const selecting = sync.select('group');

  summaries.resolve({ items: [group()], next_cursor: null });
  await starting;
  await selecting;

  expect(sync.messages.map((record) => record.id)).toEqual(['old']);
  expect(sync.appliedSeq).toBe('1');
});

test('current membership invalidation purges state while an older hint cannot purge a rejoin', async () => {
  let current: Group | undefined = group();
  const { sync, transport } = fixture({
    list: async () => ({ items: current ? [current] : [], next_cursor: null }),
    snapshot: async () => ({
      items: [message('visible', current?.latest_event_seq)],
      snapshot_seq: current?.latest_event_seq ?? '1',
      next_before: null,
      membership: current?.membership,
    }),
  });
  await sync.start();
  await sync.select('group');

  current = undefined;
  transport.receive({
    kind: 'membership.invalidated',
    conversation_id: 'group',
    membership_id: 'old',
  });
  await expect.poll(() => sync.messages.length).toBe(0);
  current = group('new', '10');
  await sync.refreshSummaries();
  await sync.select('group');
  transport.receive({
    kind: 'membership.invalidated',
    conversation_id: 'group',
    membership_id: 'old',
  });

  expect(sync.messages.map((item) => item.id)).toEqual(['visible']);
  expect(sync.appliedSeq).toBe('10');
});

test('a delayed send from an old membership cannot populate the new membership cache', async () => {
  let current = group();
  const sent = deferred<Message>();
  const { sync } = fixture({
    list: async () => ({ items: [current], next_cursor: null }),
    send: () => sent.promise,
    snapshot: async () => ({
      items: [],
      snapshot_seq: current.latest_event_seq,
      next_before: null,
      membership: current.membership,
    }),
  });
  await sync.start();
  await sync.select('group');
  const sending = sync.send('late', 'late');

  current = group('new', '10');
  await sync.refreshSummaries();
  await sync.select('group');
  sent.resolve({ ...message('late', '2'), sender_id: 'alice' });
  await sending;

  expect(sync.messages).toEqual([]);
  expect(sync.currentPending).toEqual([]);
  expect(sync.appliedSeq).toBe('10');
});

test('group replay advances contiguously above the safe integer limit', async () => {
  const first = '9007199254740993';
  const second = '9007199254740994';
  const third = '9007199254740995';
  const current = group('large', first);
  const requests: string[] = [];
  const { sync, transport } = fixture({
    list: async () => ({ items: [current], next_cursor: null }),
    snapshot: async () => ({
      items: [],
      snapshot_seq: first,
      next_before: null,
      membership: current.membership,
    }),
    events: async (_id, after) => {
      requests.push(after);
      const next = (BigInt(after) + 1n).toString();
      const record = message(next, next);
      return {
        events: [
          {
            envelope: {
              schema_version: 1,
              event_id: next,
              kind: 'message.created',
              conversation_id: 'group',
              event_seq: next,
            },
            payload: { type: 'message.created', data: record },
          },
        ],
        last_seq: next,
        snapshot_seq: third,
        has_more: next !== third,
        membership: current.membership,
      };
    },
  });
  await sync.start();
  await sync.select('group');

  transport.receive({
    schema_version: 1,
    event_id: 'hint',
    kind: 'message.created',
    conversation_id: 'group',
    event_seq: third,
  });
  await expect.poll(() => sync.appliedSeq).toBe(third);

  expect(requests).toEqual([first, second]);
  expect(sync.messages.map((record) => record.created_seq)).toEqual([
    second,
    third,
  ]);
});

test('an old read response cannot advance or refresh a rejoined episode', async () => {
  let current = group();
  const oldRead = deferred<{ my_read_seq: string }>();
  let reads = 0;
  let lists = 0;
  const { sync } = fixture({
    list: async () => {
      lists++;
      return { items: [current], next_cursor: null };
    },
    snapshot: async () => ({
      items: [message('readable', current.latest_event_seq)],
      snapshot_seq: current.latest_event_seq,
      next_before: null,
      membership: current.membership,
    }),
    read: async () => {
      reads++;
      return oldRead.promise;
    },
  });
  await sync.start();
  await sync.select('group');
  sync.setVisible(true);
  await expect.poll(() => reads).toBe(1);

  current = group('new', '10');
  await sync.refreshSummaries();
  await sync.select('group');
  const before = lists;
  oldRead.resolve({ my_read_seq: '999' });
  await oldRead.promise;
  await Promise.resolve();

  expect(lists).toBe(before);
  expect(sync.current?.my_read_seq).toBe('0');
  expect(sync.appliedSeq).toBe('10');
});

test('a summary request authorized before removal cannot resurrect the revoked group', async () => {
  const delayed = deferred<{ items: Group[]; next_cursor: null }>();
  let calls = 0;
  const { sync, transport } = fixture({
    list: async () =>
      ++calls === 1
        ? { items: [group()], next_cursor: null }
        : calls === 2
          ? delayed.promise
          : { items: [], next_cursor: null },
  });
  await sync.start();
  await sync.select('group');
  const refreshing = sync.refreshSummaries();
  await expect.poll(() => calls).toBe(2);

  transport.receive({
    kind: 'membership.invalidated',
    conversation_id: 'group',
    membership_id: 'old',
  });
  delayed.resolve({ items: [group()], next_cursor: null });
  await refreshing;

  expect(sync.conversations).toEqual([]);
  expect(sync.messages).toEqual([]);
});
