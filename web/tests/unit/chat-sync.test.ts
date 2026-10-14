import { createQueryClient } from '../../src/lib/features/chat/cache';
import { afterEach, expect, test } from 'vitest';
import {
  createChatSync,
  type ChatSync,
} from '../../src/lib/features/chat/sync.svelte';
import {
  type ChatApi,
  type Message,
  type Event,
  type Snapshot,
  type RealtimeEvent,
} from '../../src/lib/features/chat/models';
import type { RealtimeTransport } from '../../src/lib/features/chat/realtime';
const first = '9007199254740993',
  second = '9007199254740994';
function message(id: string, seq: string, conversation = 'a'): Message {
  return {
    id,
    created_seq: seq,
    revision_seq: seq,
    withdrawn: false,
    conversation_id: conversation,
    client_message_id: id,
    sender_id: 'alice',
    body: id,
    created_at: '2026-10-12T00:00:00Z',
  };
}
function event(msg: Message): Event {
  return {
    envelope: {
      schema_version: 1,
      event_id: msg.id,
      kind: 'message.created',
      conversation_id: msg.conversation_id,
      event_seq: msg.created_seq,
    },
    payload: { type: 'message.created', data: msg },
  };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
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
const running: ChatSync[] = [];
afterEach(() => {
  for (const sync of running) sync.stop();
  running.length = 0;
});
function fixture(overrides: Partial<ChatApi> = {}) {
  const api: ChatApi = {
    list: async () => ({ items: [], next_cursor: null }),
    conversation: async () => {
      throw new Error('not needed');
    },
    snapshot: async () => ({ items: [], snapshot_seq: '1', next_before: null }),
    events: async (_id, after) => ({
      events: [],
      last_seq: after,
      snapshot_seq: after,
      has_more: false,
    }),
    send: async () => {
      throw new Error('offline');
    },
    edit: async () => {
      throw new Error('not needed');
    },
    withdraw: async () => {
      throw new Error('not needed');
    },
    search: async () => ({ items: [], next_before: null, snapshot_seq: '1' }),
    context: async () => ({
      items: [],
      target_message_id: '',
      next_before: null,
      next_after: null,
      snapshot_seq: '1',
    }),
    preferences: async () => ({ pinned: false, muted: false, archived: false }),
    read: async (_id, seq) => ({ my_read_seq: seq }),
    typing: async () => {},
    status: async () => ({ online: false }),
    members: async () => [],
    ...overrides,
  };
  const transport = new Transport();
  const queryClient = createQueryClient();
  const sync = createChatSync({ api, transport, queryClient, userId: 'alice' });
  running.push(sync);
  return { api, sync, transport, queryClient };
}
test('notification during snapshot is replayed using precise BigInt sequences', async () => {
  const snapshot = deferred<Snapshot>();
  const m1 = message('m1', first),
    m2 = message('m2', second);
  const { sync, transport } = fixture({
    snapshot: () => snapshot.promise,
    events: async () => ({
      events: [event(m2)],
      last_seq: second,
      snapshot_seq: second,
      has_more: false,
    }),
  });
  await sync.start();
  const loading = sync.select('a');
  transport.receive(event(m2).envelope);
  snapshot.resolve({ items: [m1], snapshot_seq: first, next_before: null });
  await loading;
  await expect.poll(() => sync.appliedSeq).toBe(second);
  expect(sync.messages.map((m) => m.id)).toEqual(['m1', 'm2']);
});
test('duplicate and out-of-order notifications apply a durable event once', async () => {
  const m = message('m2', '2');
  const { sync, transport } = fixture({
    events: async () => ({
      events: [event(m)],
      last_seq: '2',
      snapshot_seq: '2',
      has_more: false,
    }),
  });
  await sync.start();
  await sync.select('a');
  transport.receive(event(m).envelope);
  transport.receive(event(m).envelope);
  await expect.poll(() => sync.appliedSeq).toBe('2');
  transport.receive({ ...event(m).envelope, event_seq: '1' });
  expect(sync.messages).toHaveLength(1);
});
test('an event gap fetches every page instead of jumping to the notification sequence', async () => {
  const requests: string[] = [];
  const { sync, transport } = fixture({
    events: async (_id, after) => {
      requests.push(after);
      const next = (BigInt(after) + 1n).toString();
      return {
        events: [event(message(`m${next}`, next))],
        last_seq: next,
        snapshot_seq: '4',
        has_more: next !== '4',
      };
    },
  });
  await sync.start();
  await sync.select('a');
  transport.receive(event(message('m4', '4')).envelope);
  await expect.poll(() => sync.appliedSeq).toBe('4');
  expect(requests).toEqual(['1', '2', '3']);
  expect(sync.messages.map((m) => m.created_seq)).toEqual(['2', '3', '4']);
});
test('an old snapshot and another conversation notification cannot mutate the selected conversation', async () => {
  const old = deferred<Snapshot>();
  const { sync, transport, queryClient } = fixture({
    snapshot: (id) =>
      id === 'a'
        ? old.promise
        : Promise.resolve({
            items: [message('b-message', '2', 'b')],
            snapshot_seq: '2',
            next_before: null,
          }),
  });
  await sync.start();
  const pending = sync.select('a');
  transport.receive(event(message('other', '8', 'b')).envelope);
  await sync.select('b');
  old.resolve({
    items: [message('old', '3')],
    snapshot_seq: '3',
    next_before: null,
  });
  await pending;
  expect(sync.selectedId).toBe('b');
  expect(sync.messages.map((m) => m.id)).toEqual(['b-message']);
  expect(queryClient.getQueryData(['messages', 'a'])).toBeUndefined();
});
test('retry keeps the same client message ID and merges the saved authoritative record', async () => {
  const requests: { client_message_id: string; body: string }[] = [];
  const { sync } = fixture({
    send: async (id, input) => {
      requests.push(input);
      if (requests.length === 1) throw new Error('response lost');
      return {
        ...message('server', '2', id),
        body: input.body,
        client_message_id: input.client_message_id,
      };
    },
  });
  await sync.start();
  await sync.select('a');
  await sync.send('hello', 'client-id');
  expect(sync.pending[0].status).toBe('failed');
  await sync.retry('client-id');
  expect(requests).toEqual([
    { client_message_id: 'client-id', body: 'hello' },
    { client_message_id: 'client-id', body: 'hello' },
  ]);
  expect(sync.pending).toHaveLength(0);
  expect(sync.messages[0].id).toBe('server');
});

test('file-only retries freeze the ordered attachment IDs with the original client message ID', async () => {
  const requests: unknown[] = [];
  const ids = ['second-file', 'first-file'];
  const { sync } = fixture({
    send: async (id, input) => {
      requests.push(structuredClone(input));
      if (requests.length === 1) throw new Error('response lost');
      return {
        ...message('saved-file', '2', id),
        body: input.body,
        client_message_id: input.client_message_id,
      };
    },
  });
  await sync.start();
  await sync.select('a');
  await sync.send('', 'file-client', ids);
  ids.reverse();

  await sync.retry('file-client');

  expect(requests).toEqual([
    {
      client_message_id: 'file-client',
      body: '',
      attachment_ids: ['second-file', 'first-file'],
      membership_id: undefined,
    },
    {
      client_message_id: 'file-client',
      body: '',
      attachment_ids: ['second-file', 'first-file'],
      membership_id: undefined,
    },
  ]);
});
test('a hidden page never submits a read cursor', async () => {
  const reads: string[] = [];
  const { sync } = fixture({
    snapshot: async () => ({
      items: [message('m', '2')],
      snapshot_seq: '2',
      next_before: null,
    }),
    read: async (id, seq) => {
      reads.push(id);
      return { my_read_seq: seq };
    },
  });
  await sync.start();
  await sync.select('a');
  sync.setVisible(false);
  expect(reads).toEqual([]);
  sync.setVisible(true);
  await expect.poll(() => reads).toEqual(['a']);
});

test('a successful send cannot acknowledge an incoming message before continuous replay and rendering', async () => {
  const replay = deferred<Awaited<ReturnType<ChatApi['events']>>>();
  const reads: string[] = [];
  const incoming = { ...message('incoming', '2'), sender_id: 'bob' };
  const own = message('own', '3');
  const { sync } = fixture({
    events: () => replay.promise,
    send: async () => own,
    read: async (_id, cursor) => {
      reads.push(cursor);
      return { my_read_seq: cursor };
    },
  });
  await sync.start();
  await sync.select('a');
  await sync.send('own', 'own');
  expect(sync.messages.map((m) => m.id)).toEqual(['own']);
  expect(sync.appliedSeq).toBe('1');
  sync.setVisible(true); // The own message has rendered; the preceding incoming message has not.
  expect(reads).toEqual([]);
  sync.setVisible(false); // MessageList invalidates visibility during each render.
  replay.resolve({
    events: [event(incoming), event(own)],
    last_seq: '3',
    snapshot_seq: '3',
    has_more: false,
  });
  await expect.poll(() => sync.appliedSeq).toBe('3');
  expect(sync.messages.map((m) => m.id)).toEqual(['incoming', 'own']);
  expect(reads).toEqual([]);
  sync.setVisible(true); // MessageList confirms visibility only after the updated DOM renders.
  await expect.poll(() => reads).toEqual(['3']);
});

test('completion of an earlier receipt only acknowledges the last rendered cursor', async () => {
  const firstRead = deferred<{ my_read_seq: string }>();
  const reads: string[] = [];
  const incoming = { ...message('incoming', '3'), sender_id: 'bob' };
  const { sync, transport } = fixture({
    snapshot: async () => ({
      items: [message('old', '2')],
      snapshot_seq: '2',
      next_before: null,
    }),
    events: async () => ({
      events: [event(incoming)],
      last_seq: '3',
      snapshot_seq: '3',
      has_more: false,
    }),
    read: async (_id, cursor) => {
      reads.push(cursor);
      return cursor === '2' ? firstRead.promise : { my_read_seq: cursor };
    },
  });
  await sync.start();
  await sync.select('a');
  sync.setVisible(true);
  transport.receive(event(incoming).envelope);
  await expect.poll(() => sync.appliedSeq).toBe('3');
  firstRead.resolve({ my_read_seq: '2' });
  await Promise.resolve();
  await Promise.resolve();
  expect(reads).toEqual(['2']); // Replay completed; the visibility callback has not confirmed the new DOM.
  sync.setVisible(true);
  await expect.poll(() => reads).toEqual(['2', '3']);
});
