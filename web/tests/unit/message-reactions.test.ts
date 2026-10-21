import { afterEach, expect, test, vi } from 'vitest';

import {
  createChatSync,
  type ChatSync,
} from '../../src/lib/features/chat/sync.svelte';
import { createQueryClient } from '../../src/lib/features/chat/cache';
import { mergeMessageRecords } from '../../src/lib/features/chat/messages/message-records';
import type {
  ChatApi,
  Conversation,
  Message,
  Event,
} from '../../src/lib/features/chat/models';
import type { RealtimeTransport } from '../../src/lib/features/chat/synchronization/realtime';

const running: ChatSync[] = [];
afterEach(() => running.splice(0).forEach((sync) => sync.stop()));

function message(revision = '2'): Message {
  return {
    id: 'target',
    conversation_id: 'chat',
    client_message_id: 'target',
    sender_id: 'bob',
    body: '消息',
    created_at: '2026-10-14T00:00:00Z',
    created_seq: '2',
    revision_seq: revision,
    withdrawn: false,
    reactions: [],
  };
}

function fixture(overrides: Partial<ChatApi> = {}) {
  const conversation: Conversation = {
    id: 'chat',
    peer: { id: 'bob', username: 'bob', display_name: '伙伴' },
    latest_event_seq: '2',
    last_message: null,
    unread_count: 1,
    my_read_seq: '0',
    peer_read_seq: '0',
    preferences: { muted: false, pinned: false, archived: false },
  };
  const read = vi.fn<ChatApi['read']>(async (_, seq) => ({ my_read_seq: seq }));
  const setReaction = vi.fn(async () => ({
    ...message('3'),
    reactions: [{ kind: 'like' as const, count: 1, mine: true }],
  }));
  const api: ChatApi = {
    list: async () => ({ items: [conversation], next_cursor: null }),
    conversation: async () => conversation,
    snapshot: async () => ({
      items: [message()],
      next_before: null,
      snapshot_seq: '2',
    }),
    events: async (_, after) => ({
      events: [],
      last_seq: after,
      snapshot_seq: after,
      has_more: false,
    }),
    send: async () => message(),
    edit: async () => message(),
    withdraw: async () => message(),
    setReaction,
    removeReaction: async () => message('3'),
    search: async () => ({ items: [], next_before: null, snapshot_seq: '2' }),
    context: async () => ({
      items: [],
      target_message_id: 'target',
      next_before: null,
      next_after: null,
      snapshot_seq: '2',
    }),
    preferences: async () => conversation.preferences,
    members: async () => [],
    read,
    typing: async () => {},
    status: async () => ({ online: false }),
    ...overrides,
  };
  let receive: Parameters<RealtimeTransport['subscribe']>[0] = () => {};
  const transport: RealtimeTransport = {
    subscribe: async (callback) => {
      receive = callback;
    },
    close: () => {},
  };
  const sync = createChatSync({
    api,
    queryClient: createQueryClient(),
    transport,
    userId: 'alice',
  });
  running.push(sync);
  return {
    sync,
    api,
    read,
    setReaction,
    receive: (event: Parameters<typeof receive>[0]) => receive(event),
  };
}

test('only one pending choice per message applies authoritative counts without acknowledging a read', async () => {
  let resolve!: (message: Message) => void;
  const fixtureState = fixture({
    setReaction: vi.fn(() => new Promise<Message>((done) => (resolve = done))),
  });
  await fixtureState.sync.start();
  await fixtureState.sync.select('chat');

  const first = fixtureState.sync.actions.reaction('target', 'like');
  await fixtureState.sync.actions.reaction('target', 'love');
  expect(fixtureState.api.setReaction).toHaveBeenCalledOnce();
  expect(fixtureState.sync.actions.reactionPending('target')).toBe(true);
  resolve({
    ...message('3'),
    reactions: [{ kind: 'like', count: 1, mine: true }],
  });
  await first;

  expect(fixtureState.sync.messages[0].reactions).toEqual([
    { kind: 'like', count: 1, mine: true },
  ]);
  expect(fixtureState.sync.actions.reactionPending('target')).toBe(false);
  expect(fixtureState.read).not.toHaveBeenCalled();
  expect(fixtureState.sync.notifications.items).toEqual([]);
});

test('late acknowledgements cannot apply after the authenticated lifetime or selection changed', async () => {
  let resolve!: (message: Message) => void;
  const fixtureState = fixture({
    setReaction: () => new Promise<Message>((done) => (resolve = done)),
  });
  await fixtureState.sync.start();
  await fixtureState.sync.select('chat');
  const pending = fixtureState.sync.actions.reaction('target', 'like');

  fixtureState.sync.actions.reset();
  resolve({
    ...message('3'),
    reactions: [{ kind: 'like', count: 1, mine: true }],
  });
  await pending;

  expect(fixtureState.sync.messages[0].reactions).toEqual([]);
  expect(fixtureState.read).not.toHaveBeenCalled();
});

test('continuous reaction replay advances only the event cursor and creates no new-message alert', async () => {
  const updated = {
    ...message('3'),
    reactions: [{ kind: 'love' as const, count: 1, mine: false }],
  };
  const event: Event = {
    envelope: {
      schema_version: 1,
      event_id: 'reaction',
      event_seq: '3',
      conversation_id: 'chat',
      kind: 'message.reactions_updated',
    },
    payload: {
      type: 'message.reactions_updated',
      data: { message_id: 'target', message: updated },
    },
  };
  const fixtureState = fixture({
    events: async () => ({
      events: [event],
      last_seq: '3',
      snapshot_seq: '3',
      has_more: false,
    }),
  });
  await fixtureState.sync.start();
  await fixtureState.sync.select('chat');

  fixtureState.receive(event.envelope);
  await expect.poll(() => fixtureState.sync.appliedSeq).toBe('3');

  expect(fixtureState.sync.messages[0].reactions).toEqual(updated.reactions);
  expect(fixtureState.sync.notifications.items).toEqual([]);
  expect(fixtureState.read).not.toHaveBeenCalled();
});

test('newer reaction revisions survive stale history and a withdrawn target has no visible reactions', () => {
  const newer = {
    ...message('9007199254740994'),
    reactions: [{ kind: 'love' as const, count: 2, mine: true }],
  };

  const merged = mergeMessageRecords([newer], [message('9007199254740993')]);

  expect(merged[0].reactions).toEqual(newer.reactions);
  const withdrawn = {
    ...newer,
    revision_seq: '9007199254740995',
    withdrawn: true,
    body: '',
    reactions: [],
  };
  expect(mergeMessageRecords([withdrawn], [newer])[0].reactions).toEqual([]);
});

test('a reaction failure leaves authoritative counts unchanged and exposes retryable feedback', async () => {
  const fixtureState = fixture({
    setReaction: async () => {
      throw new Error('Retry reaction');
    },
  });
  await fixtureState.sync.start();
  await fixtureState.sync.select('chat');

  await fixtureState.sync.actions.reaction('target', 'like');

  expect(fixtureState.sync.messages[0].reactions).toEqual([]);
  expect(fixtureState.sync.error).toBe('Retry reaction');
  expect(fixtureState.sync.actions.reactionPending('target')).toBe(false);
  expect(fixtureState.read).not.toHaveBeenCalled();
});

test('a late choice from a different selected conversation cannot replace its cached projection', async () => {
  let resolve!: (message: Message) => void;
  const fixtureState = fixture({
    setReaction: () => new Promise<Message>((done) => (resolve = done)),
  });
  await fixtureState.sync.start();
  await fixtureState.sync.select('chat');
  const pending = fixtureState.sync.actions.reaction('target', 'like');

  fixtureState.sync.selectedId = 'different';
  resolve({
    ...message('3'),
    reactions: [{ kind: 'like', count: 1, mine: true }],
  });
  await pending;
  fixtureState.sync.selectedId = 'chat';

  expect(fixtureState.sync.messages[0].reactions).toEqual([]);
  expect(fixtureState.read).not.toHaveBeenCalled();
});
