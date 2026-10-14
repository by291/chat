import type { RealtimeEvent } from './models';
import { Centrifuge } from 'centrifuge';

export type ConnectionStatus = 'connecting' | 'online' | 'offline';

export interface RealtimeTransport {
  subscribe(
    onEvent: (event: RealtimeEvent) => void,
    onReconnect: () => void,
    onStatus?: (state: ConnectionStatus) => void,
  ): Promise<void>;
  close(): void;
}

function isEvent(data: unknown): data is RealtimeEvent {
  if (!data || typeof data !== 'object') {
    return false;
  }
  const candidate = data as Record<string, unknown>;
  if (candidate.kind === 'conversation.preferences.updated')
    return typeof candidate.conversation_id === 'string';
  if (candidate.kind === 'membership.invalidated') {
    return (
      typeof candidate.conversation_id === 'string' &&
      typeof candidate.membership_id === 'string'
    );
  }
  if (candidate.kind === 'presence.changed') {
    return typeof candidate.user_id === 'string';
  }
  if (candidate.kind === 'typing.changed') {
    return (
      typeof candidate.conversation_id === 'string' &&
      typeof candidate.user_id === 'string' &&
      typeof candidate.is_typing === 'boolean' &&
      typeof candidate.expires_at === 'string' &&
      Number.isFinite(Date.parse(candidate.expires_at))
    );
  }
  return (
    [
      'conversation.created',
      'message.created',
      'message.edited',
      'message.withdrawn',
      'receipt.updated',
      'group.updated',
      'membership.updated',
    ].includes(String(candidate.kind)) &&
    candidate.schema_version === 1 &&
    typeof candidate.event_id === 'string' &&
    typeof candidate.conversation_id === 'string' &&
    typeof candidate.event_seq === 'string' &&
    /^(0|[1-9][0-9]*)$/.test(candidate.event_seq) &&
    BigInt(candidate.event_seq) <= 9223372036854775807n
  );
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
        if (isEvent(context.data)) {
          onEvent(context.data);
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
      await subscription.ready(5000);
    },
    close() {
      client?.disconnect();
      client = null;
    },
  };
}
