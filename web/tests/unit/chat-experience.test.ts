import { afterEach, expect, test } from 'vitest';

import { createQueryClient } from '../../src/lib/features/chat/cache';
import { ConversationStore } from '../../src/lib/features/chat/conversation-store';
import { mergeMessageRecords } from '../../src/lib/features/chat/message-records';
import {
  createChatSync,
  type ChatSync,
} from '../../src/lib/features/chat/sync.svelte';
import type {
  ChatApi,
  Conversation,
  Event,
  Group,
  Message,
  RealtimeEvent,
} from '../../src/lib/features/chat/models';
import type { RealtimeTransport } from '../../src/lib/features/chat/realtime';

const running: ChatSync[] = [];

afterEach(() => {
  running.splice(0).forEach((sync) => sync.stop());
});

function message(
  id = 'original',
  revision = '2',
  body = '最初的消息',
): Message {
  return {
    id,
    conversation_id: 'chat',
    client_message_id: id,
    sender_id: 'bob',
    body,
    created_at: '2026-10-14T00:00:00Z',
    created_seq: '2',
    revision_seq: revision,
    withdrawn: false,
    reactions: [],
  };
}

function conversation(): Conversation {
  return {
    id: 'chat',
    peer: { id: 'bob', username: 'bob', display_name: '伙伴' },
    latest_event_seq: '2',
    last_message: null,
    unread_count: 0,
    my_read_seq: '0',
    peer_read_seq: '0',
    preferences: { pinned: false, muted: false, archived: false },
  };
}

function event(
  record: Message,
  eventSeq: string,
  type:
    | 'message.created'
    | 'message.edited'
    | 'message.withdrawn' = 'message.created',
): Event {
  return {
    envelope: {
      schema_version: 1,
      event_id: eventSeq,
      conversation_id: 'chat',
      event_seq: eventSeq,
      kind: type,
    },
    payload: { type, data: record },
  };
}

class Transport implements RealtimeTransport {
  receive: (event: RealtimeEvent) => void = () => {};
  status: (status: 'online' | 'offline' | 'connecting') => void = () => {};
  async subscribe(
    receive: (event: RealtimeEvent) => void,
    _reconnect: () => void,
    status?: typeof this.status,
  ) {
    this.receive = receive;
    this.status = status ?? (() => {});
  }
  close() {}
}

function fixture(overrides: Partial<ChatApi> = {}) {
  const api: ChatApi = {
    list: async () => ({ items: [conversation()], next_cursor: null }),
    conversation: async () => conversation(),
    snapshot: async () => ({
      items: [message()],
      snapshot_seq: '2',
      next_before: null,
    }),
    events: async (_id, after) => ({
      events: [],
      last_seq: after,
      snapshot_seq: after,
      has_more: false,
    }),
    send: async () => message(),
    edit: async () => message(),
    withdraw: async () => message(),
    setReaction: async () => message(),
    removeReaction: async () => message(),
    search: async () => ({ items: [], next_before: null, snapshot_seq: '2' }),
    context: async () => ({
      items: [],
      target_message_id: 'original',
      next_before: null,
      next_after: null,
      snapshot_seq: '2',
    }),
    preferences: async () => conversation().preferences,
    members: async () => [],
    read: async (_id, seq) => ({ my_read_seq: seq }),
    typing: async () => {},
    status: async () => ({ online: false }),
    ...overrides,
  };
  const queryClient = createQueryClient();
  const transport = new Transport();
  const sync = createChatSync({ api, queryClient, transport, userId: 'alice' });
  running.push(sync);
  return { sync, api, transport, queryClient };
}

function store() {
  return new ConversationStore({
    ...fixture(),
    userId: 'alice',
    active: () => true,
    epoch: () => 1,
    selectedId: () => 'chat',
    changed: () => {},
    failed: () => {},
    messagesMerged: () => {},
    peerSelected: () => {},
    membershipChanged: () => {},
  });
}

test('stale acknowledgements and history cannot resurrect a withdrawn message above Number precision', () => {
  const records = store();
  const withdrawn = {
    ...message('original', '9007199254740994', ''),
    withdrawn: true,
  };
  records.merge('chat', [withdrawn]);

  records.merge('chat', [message('original', '9007199254740993')]);

  expect(records.messages('chat')[0]).toEqual(withdrawn);
});

test('reply references update when a target changes and retain their own newer revision in stale responses', () => {
  const records = store();
  const original = message();
  const reply = {
    ...message('reply', '3', '回复'),
    created_seq: '3',
    reply_to_message_id: original.id,
    reply: {
      id: original.id,
      sender_id: 'bob',
      created_seq: '2',
      revision_seq: '2',
      body: original.body,
      withdrawn: false,
    },
  };
  records.merge('chat', [original, reply]);
  records.merge('chat', [
    { ...original, revision_seq: '4', body: '', withdrawn: true },
  ]);

  records.merge('chat', [reply]);

  expect(records.messages('chat')[1].revision_seq).toBe('3');
  expect(records.messages('chat')[1].reply).toMatchObject({
    revision_seq: '4',
    body: '',
    withdrawn: true,
  });
});

test('a stale summary cannot reveal an already withdrawn last message', async () => {
  const summary = { ...conversation(), last_message: message() };
  const { sync } = fixture({
    list: async () => ({ items: [summary], next_cursor: null }),
    snapshot: async () => ({
      items: [{ ...message('original', '3', ''), withdrawn: true }],
      snapshot_seq: '3',
      next_before: null,
    }),
  });
  await sync.start();
  await sync.select('chat');

  await sync.refreshSummaries();

  expect(sync.current?.last_message).toMatchObject({
    revision_seq: '3',
    body: '',
    withdrawn: true,
  });
});

test('private-chat replies and search use the known peer display name', async () => {
  const { sync } = fixture();
  await sync.start();
  await sync.select('chat');

  expect(sync.senderName('bob')).toBe('伙伴');
  expect(sync.senderName('alice')).toBe('你');
});

test('context navigation never marks unseen newer history read and leaves the applied cursor unchanged', async () => {
  const reads: string[] = [];
  const { sync } = fixture({
    context: async () => ({
      target_message_id: 'old',
      items: [{ ...message('old'), created_seq: '1' }],
      next_before: null,
      next_after: '1',
      snapshot_seq: '99',
    }),
    read: async (_id, cursor) => {
      reads.push(cursor);
      return { my_read_seq: cursor };
    },
  });
  await sync.start();
  await sync.select('chat');

  await sync.history.jump('old');
  sync.setVisible(true);

  expect(sync.appliedSeq).toBe('2');
  expect(sync.messages.map((record) => record.id)).toEqual(['old']);
  expect(reads).toEqual([]);
  await sync.history.latest();
  sync.setVisible(true);
  await expect.poll(() => reads).toEqual(['2']);
});

test('replay edits and withdrawals advance continuously and never notify', async () => {
  const withdrawn = { ...message('original', '4', ''), withdrawn: true };
  const { sync, transport } = fixture({
    events: async () => ({
      events: [
        event(message('original', '3', '改好了'), '3', 'message.edited'),
        event(withdrawn, '4', 'message.withdrawn'),
      ],
      last_seq: '4',
      snapshot_seq: '4',
      has_more: false,
    }),
  });
  await sync.start();
  await sync.select('chat');

  transport.receive(event(withdrawn, '4', 'message.withdrawn').envelope);

  await expect.poll(() => sync.appliedSeq).toBe('4');
  expect(sync.messages[0].withdrawn).toBe(true);
  expect(sync.notifications.items).toEqual([]);
});

test('retry freezes the original reply ID even after the draft reply target changes', async () => {
  const requests: unknown[] = [];
  const { sync } = fixture({
    send: async (_id, input) => {
      requests.push(structuredClone(input));
      if (requests.length === 1) throw new Error('response lost');
      return {
        ...message('sent'),
        sender_id: 'alice',
        client_message_id: input.client_message_id,
      };
    },
  });
  await sync.start();
  await sync.select('chat');
  sync.actions.reply(message());
  await sync.send('回复', 'retry');
  sync.actions.reply(message('different'));

  await sync.retry('retry');

  expect(requests).toEqual([
    {
      client_message_id: 'retry',
      body: '回复',
      membership_id: undefined,
      reply_to_message_id: 'original',
    },
    {
      client_message_id: 'retry',
      body: '回复',
      membership_id: undefined,
      reply_to_message_id: 'original',
    },
  ]);
});

test('only the exact fresh incoming publication notifies, once, without acknowledging a read cursor', async () => {
  const backlog = {
    ...message('backlog'),
    created_seq: '3',
    revision_seq: '3',
  };
  const fresh = { ...message('fresh'), created_seq: '4', revision_seq: '4' };
  const reads: string[] = [];
  const { sync, transport } = fixture({
    events: async () => ({
      events: [event(backlog, '3'), event(fresh, '4')],
      last_seq: '4',
      snapshot_seq: '4',
      has_more: false,
    }),
    read: async (_id, cursor) => {
      reads.push(cursor);
      return { my_read_seq: cursor };
    },
  });
  await sync.start();

  transport.receive(event(fresh, '4').envelope);
  transport.receive(event(fresh, '4').envelope);

  await expect
    .poll(() => sync.notifications?.items.map((item) => item.messageId) ?? [])
    .toEqual(['fresh']);
  expect(reads).toEqual([]);
});

test.each(['self', 'muted', 'focused', 'offline'] as const)(
  '%s messages remain silent',
  async (reason) => {
    const fresh = {
      ...message('fresh'),
      created_seq: '3',
      revision_seq: '3',
      sender_id: reason === 'self' ? 'alice' : 'bob',
    };
    const summary = conversation();
    summary.preferences.muted = reason === 'muted';
    const { sync, transport } = fixture({
      list: async () => ({ items: [summary], next_cursor: null }),
      events: async () => ({
        events: [event(fresh, '3')],
        last_seq: '3',
        snapshot_seq: '3',
        has_more: false,
      }),
    });
    await sync.start();
    await sync.select('chat');
    if (reason === 'focused') sync.setFocused(true);
    if (reason === 'offline') transport.status('offline');

    transport.receive(event(fresh, '3').envelope);

    await expect.poll(() => sync.appliedSeq).toBe('3');
    expect(sync.notifications?.items ?? []).toEqual([]);
  },
);

test('reconnect publications remain silent until authoritative reconciliation finishes', async () => {
  let release!: (value: { items: Conversation[]; next_cursor: null }) => void;
  const summaries = new Promise<{ items: Conversation[]; next_cursor: null }>(
    (resolve) => {
      release = resolve;
    },
  );
  let lists = 0;
  const missed = { ...message('missed'), created_seq: '3', revision_seq: '3' };
  const { sync, transport } = fixture({
    list: async () =>
      ++lists === 1
        ? { items: [conversation()], next_cursor: null }
        : summaries,
    events: async () => ({
      events: [event(missed, '3')],
      last_seq: '3',
      snapshot_seq: '3',
      has_more: false,
    }),
  });
  await sync.start();
  await sync.select('chat');
  transport.status('offline');
  transport.status('online');

  transport.receive(event(missed, '3').envelope);
  await expect.poll(() => sync.appliedSeq).toBe('3');
  release({
    items: [{ ...conversation(), latest_event_seq: '3' }],
    next_cursor: null,
  });
  await summaries;

  expect(sync.notifications.items).toEqual([]);
});

test('leaving a conversation cancels old work but permits fresh background replay and alerts', async () => {
  const fresh = {
    ...message('background'),
    created_seq: '3',
    revision_seq: '3',
  };
  const { sync, transport } = fixture({
    events: async (_id, _after, signal) => {
      if (signal?.aborted) throw new Error('aborted');
      return {
        events: [event(fresh, '3')],
        last_seq: '3',
        snapshot_seq: '3',
        has_more: false,
      };
    },
  });
  await sync.start();
  await sync.select('chat');
  sync.clearSelection();

  transport.receive(event(fresh, '3').envelope);

  await expect
    .poll(() => sync.notifications.items.map((item) => item.messageId))
    .toEqual(['background']);
});

test('changing search text does not reuse a cursor from the previous submitted query', async () => {
  const requests: [string, string | undefined][] = [];
  const { sync } = fixture({
    search: async (_id, query, before) => {
      requests.push([query, before]);
      return { items: [message()], next_before: '2', snapshot_seq: '2' };
    },
  });
  await sync.start();
  await sync.select('chat');
  sync.history.query = '最初';
  await sync.history.search();

  sync.history.query = '新查询';
  await sync.history.search(true);

  expect(sync.history.hasMoreResults).toBe(false);
  expect(requests).toEqual([['最初', undefined]]);
  await sync.history.search();
  expect(requests.at(-1)).toEqual(['新查询', undefined]);
});

test('an author can clear a file caption while retaining its attachments', async () => {
  const original: Message = {
    ...message(),
    sender_id: 'alice',
    attachments: [
      {
        id: 'file',
        filename: 'notes.txt',
        size_bytes: 12,
        content_type: 'text/plain',
        purpose: 'message',
        state: 'ready',
      },
    ],
  };
  const edits: string[] = [];
  const { sync } = fixture({
    snapshot: async () => ({
      items: [original],
      snapshot_seq: '2',
      next_before: null,
    }),
    edit: async (_id, _messageId, body) => {
      edits.push(body);
      return { ...original, body, revision_seq: '3' };
    },
  });
  await sync.start();
  await sync.select('chat');
  sync.actions.edit(original);
  sync.actions.editBody = '';

  await sync.actions.saveEdit();

  expect(edits).toEqual(['']);
  expect(sync.messages[0].body).toBe('');
  expect(sync.messages[0].attachments).toEqual(original.attachments);
});

function newlyAuthorized(
  kind: 'direct' | 'group',
  sequence = '2',
): Conversation {
  if (kind === 'direct')
    return { ...conversation(), latest_event_seq: sequence };
  const group: Group = {
    id: 'chat',
    kind: 'group',
    title: '新群聊',
    member_count: 2,
    membership: { membership_id: 'new-episode', joined_seq: '1' },
    my_role: 'member',
    latest_event_seq: sequence,
    my_read_seq: '0',
    unread_count: 1,
    preferences: { pinned: false, muted: false, archived: false },
  };
  return group;
}

test.each(['direct', 'group'] as const)(
  'a new %s preserves the first live candidate while its summary is in flight',
  async (kind) => {
    let release!: (value: { items: Conversation[]; next_cursor: null }) => void;
    const metadata = new Promise<{ items: Conversation[]; next_cursor: null }>(
      (resolve) => {
        release = resolve;
      },
    );
    let lists = 0;
    const summary = newlyAuthorized(kind);
    const membership =
      kind === 'group' ? (summary as Group).membership : undefined;
    const first = message('first-live');
    const requests: { after: string; membership?: string }[] = [];
    const { sync, transport } = fixture({
      list: async () =>
        ++lists === 1 ? { items: [], next_cursor: null } : metadata,
      snapshot: async () => ({
        items: [first],
        snapshot_seq: '2',
        next_before: null,
        membership,
      }),
      events: async (_id, after, _signal, membershipId) => {
        requests.push({ after, membership: membershipId });
        return {
          events: [event(first, '2')],
          last_seq: '2',
          snapshot_seq: '2',
          has_more: false,
          membership,
        };
      },
    });
    await sync.start();
    transport.receive({
      ...event(first, '1').envelope,
      kind: 'conversation.created',
    });
    transport.receive(event(first, '2').envelope);
    release({ items: [summary], next_cursor: null });

    await expect
      .poll(() => sync.notifications.items.map((item) => item.messageId))
      .toEqual(['first-live']);

    expect(requests[0]).toEqual({
      after: '1',
      membership: membership?.membership_id,
    });
    await sync.select('chat');
    expect(sync.messages[0].id).toBe('first-live');
    expect(sync.appliedSeq).toBe('2');
  },
);

test.each(['direct', 'group'] as const)(
  'a new %s metadata refetch can include the first message before its live hint',
  async (kind) => {
    let lists = 0;
    const summary = newlyAuthorized(kind);
    const membership =
      kind === 'group' ? (summary as Group).membership : undefined;
    const first = message('metadata-before-hint');
    const { sync, transport } = fixture({
      list: async () => ({
        items: ++lists === 1 ? [] : [summary],
        next_cursor: null,
      }),
      events: async () => ({
        events: [event(first, '2')],
        last_seq: '2',
        snapshot_seq: '2',
        has_more: false,
        membership,
      }),
    });
    await sync.start();
    transport.receive({
      ...event(first, '1').envelope,
      kind: 'conversation.created',
    });
    await expect.poll(() => sync.conversations.length).toBe(1);

    transport.receive(event(first, '2').envelope);

    await expect
      .poll(() => sync.notifications.items.map((item) => item.messageId))
      .toEqual(['metadata-before-hint']);
  },
);

test('initially known backlog hints cannot become first live candidates after metadata initialization', async () => {
  const current = { ...conversation(), latest_event_seq: '4' };
  const { sync, transport } = fixture({
    list: async () => ({ items: [current], next_cursor: null }),
  });
  await sync.start();

  transport.receive(event(message('old'), '3').envelope);
  await sync.refreshSummaries();

  expect(sync.notifications.items).toEqual([]);
});

test('a newer withdrawn reply preview tombstones a cached full target and fences its delayed original response', () => {
  const original: Message = {
    ...message(),
    attachments: [
      {
        id: 'file',
        filename: 'notes.txt',
        size_bytes: 12,
        content_type: 'text/plain',
        purpose: 'message',
        state: 'ready',
      },
    ],
  };
  const reply: Message = {
    ...message('reply', '3', '引用'),
    created_seq: '3',
    reply_to_message_id: original.id,
    reply: {
      id: original.id,
      sender_id: original.sender_id,
      created_seq: original.created_seq,
      revision_seq: '4',
      body: '',
      withdrawn: true,
    },
  };

  const withdrawn = mergeMessageRecords([original], [reply]);
  const delayed = mergeMessageRecords(withdrawn, [original]);

  expect(delayed.find((record) => record.id === original.id)).toMatchObject({
    body: '',
    attachments: [],
    withdrawn: true,
    revision_seq: '4',
  });
  expect(
    delayed.find((record) => record.id === original.id)?.reply,
  ).toBeUndefined();
  expect(delayed.find((record) => record.id === reply.id)?.revision_seq).toBe(
    '3',
  );
});

test('a newer edited reply preview never replaces a full target with its truncated text', () => {
  const original = message('original', '2', '完整的消息正文'.repeat(40));
  const reply: Message = {
    ...message('reply', '3', '引用'),
    created_seq: '3',
    reply_to_message_id: original.id,
    reply: {
      id: original.id,
      sender_id: original.sender_id,
      created_seq: original.created_seq,
      revision_seq: '4',
      body: '修改后的截断预览',
      withdrawn: false,
    },
  };

  const records = mergeMessageRecords([original], [reply]);

  expect(records.find((record) => record.id === original.id)?.body).toBe(
    original.body,
  );
  expect(
    records.find((record) => record.id === reply.id)?.reply?.revision_seq,
  ).toBe('4');
});

test('a late live message hint can announce its already applied record after an earlier receipt replay', async () => {
  const incoming = {
    ...message('ahead-of-hint'),
    created_seq: '4',
    revision_seq: '4',
  };
  const receipt: Event = {
    envelope: { ...event(incoming, '3').envelope, kind: 'receipt.updated' },
    payload: {
      type: 'receipt.updated',
      data: { user_id: 'bob', message_seq: '2' },
    },
  };
  const { sync, transport } = fixture({
    events: async () => ({
      events: [receipt, event(incoming, '4')],
      last_seq: '4',
      snapshot_seq: '4',
      has_more: false,
    }),
  });
  await sync.start();
  await sync.select('chat');
  transport.receive(receipt.envelope);
  await expect.poll(() => sync.appliedSeq).toBe('4');
  expect(sync.notifications.items).toEqual([]);

  transport.receive(event(incoming, '4').envelope);

  await expect
    .poll(() => sync.notifications.items.map((item) => item.messageId))
    .toEqual(['ahead-of-hint']);
  transport.receive(event(incoming, '4').envelope);
  expect(sync.notifications.items.map((item) => item.messageId)).toEqual([
    'ahead-of-hint',
  ]);
});

test('an already-read canonical message stays silent when its exact live hint arrives late', async () => {
  const incoming = {
    ...message('already-read'),
    created_seq: '4',
    revision_seq: '4',
  };
  const receipt: Event = {
    envelope: { ...event(incoming, '3').envelope, kind: 'receipt.updated' },
    payload: {
      type: 'receipt.updated',
      data: { user_id: 'bob', message_seq: '2' },
    },
  };
  let current = conversation();
  const { sync, transport } = fixture({
    list: async () => ({ items: [current], next_cursor: null }),
    events: async () => ({
      events: [receipt, event(incoming, '4')],
      last_seq: '4',
      snapshot_seq: '4',
      has_more: false,
    }),
  });
  await sync.start();
  await sync.select('chat');
  transport.receive(receipt.envelope);
  await expect.poll(() => sync.appliedSeq).toBe('4');
  current = { ...current, latest_event_seq: '4', my_read_seq: '4' };
  await sync.refreshSummaries();

  transport.receive(event(incoming, '4').envelope);

  expect(sync.notifications.items).toEqual([]);
  expect(sync.appliedSeq).toBe('4');
});

test('a successful read suppresses a late live hint while its summary refresh is still pending', async () => {
  const incoming = {
    ...message('read-before-summary'),
    created_seq: '4',
    revision_seq: '4',
  };
  const receipt: Event = {
    envelope: { ...event(incoming, '3').envelope, kind: 'receipt.updated' },
    payload: {
      type: 'receipt.updated',
      data: { user_id: 'bob', message_seq: '2' },
    },
  };
  let release!: (page: { items: Conversation[]; next_cursor: null }) => void;
  const summary = new Promise<{ items: Conversation[]; next_cursor: null }>(
    (resolve) => {
      release = resolve;
    },
  );
  let deferSummary = false;
  let pendingSummaries = 0;
  const { sync, transport } = fixture({
    list: async () => {
      if (deferSummary) {
        pendingSummaries++;
        return summary;
      }
      return { items: [conversation()], next_cursor: null };
    },
    events: async () => ({
      events: [receipt, event(incoming, '4')],
      last_seq: '4',
      snapshot_seq: '4',
      has_more: false,
    }),
    read: async (_id, cursor) => ({ my_read_seq: cursor }),
  });
  await sync.start();
  await sync.select('chat');
  transport.receive(receipt.envelope);
  await expect.poll(() => sync.appliedSeq).toBe('4');
  await sync.refreshSummaries();
  deferSummary = true;

  try {
    sync.setFocused(true);
    sync.setVisible(true);
    await expect.poll(() => pendingSummaries).toBe(1);
    expect(sync.current?.my_read_seq).toBe('0');
    sync.setVisible(false);
    sync.setFocused(false);

    transport.receive(event(incoming, '4').envelope);

    expect(sync.notifications.items).toEqual([]);
  } finally {
    release({
      items: [{ ...conversation(), latest_event_seq: '4', my_read_seq: '4' }],
      next_cursor: null,
    });
  }
});
