import { afterEach, expect, test, vi } from 'vitest';

import { createRealtimeTransport } from '../../src/lib/features/chat/realtime';

const transport = vi.hoisted(() => ({
  publication: undefined as ((context: { data: unknown }) => void) | undefined,
}));

// Keep validation and dispatch real; the SDK boundary avoids opening a browser socket.
vi.mock('centrifuge', () => ({
  Centrifuge: class {
    on() {}
    connect() {}
    disconnect() {}
    newSubscription() {
      return {
        on(event: string, listener: (context: { data: unknown }) => void) {
          if (event === 'publication') transport.publication = listener;
        },
        subscribe() {},
        ready: async () => undefined,
      };
    }
  },
}));

afterEach(() => {
  vi.unstubAllGlobals();
  transport.publication = undefined;
});

const envelope = {
  kind: 'message.created',
  schema_version: 1,
  event_id: 'event',
  conversation_id: 'conversation',
  event_seq: '9223372036854775807',
};

test('realtime dispatch preserves canonical i64 strings, supported event kinds and extension fields', async () => {
  vi.stubGlobal('location', { protocol: 'https:', host: 'chat.test' });
  const events: unknown[] = [];
  const client = createRealtimeTransport('alice', () => undefined);
  await client.subscribe(
    (event) => events.push(event),
    () => undefined,
  );
  const inputs = [
    { kind: 'contacts.updated' },
    ...[
      'conversation.created',
      'message.created',
      'message.edited',
      'message.withdrawn',
      'message.reactions_updated',
      'receipt.updated',
      'group.updated',
      'membership.updated',
    ].map((kind) => ({ ...envelope, kind })),
    { ...envelope, extension: 'future field' },
    { ...envelope, event_seq: '0' },
    { ...envelope, kind: 'receipt.updated', event_seq: '9007199254740993' },
    {
      kind: 'conversation.preferences.updated',
      conversation_id: 'conversation',
    },
    {
      kind: 'membership.invalidated',
      conversation_id: 'conversation',
      membership_id: 'episode',
    },
    { kind: 'presence.changed', user_id: 'bob' },
    {
      kind: 'typing.changed',
      conversation_id: 'conversation',
      user_id: 'bob',
      is_typing: true,
      expires_at: 'October 18, 2026 12:00:00 GMT',
    },
  ];

  for (const input of inputs) transport.publication!({ data: input });

  expect(events).toEqual(inputs);
  client.close();
});

test('realtime dispatch rejects malformed sequences, unknown versions and invalid ephemeral payloads', async () => {
  vi.stubGlobal('location', { protocol: 'https:', host: 'chat.test' });
  const events: unknown[] = [];
  const client = createRealtimeTransport('alice', () => undefined);
  await client.subscribe(
    (event) => events.push(event),
    () => undefined,
  );
  const inputs = [
    null,
    [],
    { ...envelope, schema_version: 2 },
    { ...envelope, kind: 'unknown.event' },
    ...['', '01', '-1', '1.0', '1e3', '9223372036854775808', 1].map(
      (event_seq) => ({ ...envelope, event_seq }),
    ),
    { kind: 'membership.invalidated', conversation_id: 'conversation' },
    { kind: 'presence.changed', user_id: 1 },
    {
      kind: 'typing.changed',
      conversation_id: 'conversation',
      user_id: 'bob',
      is_typing: 'true',
      expires_at: '2026-10-18T00:00:00Z',
    },
    {
      kind: 'typing.changed',
      conversation_id: 'conversation',
      user_id: 'bob',
      is_typing: true,
      expires_at: 'invalid date',
    },
  ];

  for (const input of inputs) transport.publication!({ data: input });

  expect(events).toEqual([]);
  client.close();
});
