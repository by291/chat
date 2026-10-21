import { Centrifuge } from 'centrifuge';
import { z } from 'zod';

import type { RealtimeEvent } from '../models';

const SUBSCRIPTION_READY_TIMEOUT_MS = 5_000;
const MAX_EVENT_SEQUENCE = 9_223_372_036_854_775_807n;

// Keep transport validation compatible with the generated DTOs without coercing
// exact sequences or narrowing the existing Date.parse-compatible typing expiry.
const realtimeEventSchema: z.ZodType<RealtimeEvent> = z.discriminatedUnion(
  'kind',
  [
    z.looseObject({ kind: z.literal('contacts.updated') }),
    z.looseObject({
      kind: z.enum([
        'conversation.created',
        'message.created',
        'message.edited',
        'message.withdrawn',
        'message.reactions_updated',
        'receipt.updated',
        'group.updated',
        'membership.updated',
      ]),
      schema_version: z.literal(1),
      event_id: z.string(),
      conversation_id: z.string(),
      event_seq: z
        .string()
        .regex(/^(0|[1-9][0-9]*)$/, { abort: true })
        .refine((value) => BigInt(value) <= MAX_EVENT_SEQUENCE),
    }),
    z.looseObject({
      kind: z.literal('conversation.preferences.updated'),
      conversation_id: z.string(),
    }),
    z.looseObject({
      kind: z.literal('membership.invalidated'),
      conversation_id: z.string(),
      membership_id: z.string(),
    }),
    z.looseObject({
      kind: z.literal('presence.changed'),
      user_id: z.string(),
    }),
    z.looseObject({
      kind: z.literal('typing.changed'),
      conversation_id: z.string(),
      user_id: z.string(),
      is_typing: z.boolean(),
      expires_at: z
        .string()
        .refine((value) => Number.isFinite(Date.parse(value))),
    }),
  ],
);

export type ConnectionStatus = 'connecting' | 'online' | 'offline';

export interface RealtimeTransport {
  subscribe(
    onEvent: (event: RealtimeEvent) => void,
    onReconnect: () => void,
    onStatus?: (state: ConnectionStatus) => void,
  ): Promise<void>;
  close(): void;
}

export function createRealtimeTransport(
  userId: string,
  onUnauthorized: () => void,
): RealtimeTransport {
  let client: Centrifuge | null = null;

  return {
    async subscribe(onEvent, onReconnect, onStatus) {
      client = new Centrifuge(
        `${location.protocol === 'https:' ? 'wss:' : 'ws:'}//${location.host}/connection/websocket`,
      );
      let connectedBefore = false;

      client.on('connecting', () => onStatus?.('connecting'));
      client.on('disconnected', (context) => {
        onStatus?.('offline');
        if (context.code === 4501) {
          onUnauthorized();
        }
      });

      const subscription = client.newSubscription(`personal:#${userId}`);
      subscription.on('publication', (context) => {
        const publication = realtimeEventSchema.safeParse(context.data);
        if (publication.success) {
          onEvent(publication.data);
        }
      });
      subscription.on('subscribed', () => {
        onStatus?.('online');
        if (connectedBefore) {
          onReconnect();
        }
        connectedBefore = true;
      });

      // Private subscription is established before the coordinator reads a snapshot.
      // A timeout leaves REST polling available while the SDK continues reconnecting.
      subscription.subscribe();
      client.connect();
      await subscription.ready(SUBSCRIPTION_READY_TIMEOUT_MS);
    },
    close() {
      client?.disconnect();
      client = null;
    },
  };
}
