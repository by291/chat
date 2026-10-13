import type { components } from '../../api-client';

/** Database bigint sequences arrive as canonical strings; Number would lose bits. */
export const sequence = (value: string) => BigInt(value);

export type Conversation = components['schemas']['ConversationView'];

export type Message = components['schemas']['MessageView'];

export type Envelope = components['schemas']['EventEnvelope'];

export type Event = components['schemas']['ConversationEvent'];

export type RealtimeEvent = components['schemas']['RealtimeEvent'];

export type Snapshot = components['schemas']['MessageSnapshot'];

export type EventPage = components['schemas']['EventPage'];

export interface ChatApi {
  list(
    cursor?: string,
    signal?: AbortSignal,
  ): Promise<components['schemas']['Page_ConversationView']>;
  conversation(id: string, signal?: AbortSignal): Promise<Conversation>;
  snapshot(
    id: string,
    before?: string,
    signal?: AbortSignal,
  ): Promise<Snapshot>;
  events(id: string, after: string, signal?: AbortSignal): Promise<EventPage>;
  send(
    id: string,
    input: components['schemas']['SendMessageInput'],
  ): Promise<Message>;
  read(id: string, seq: string): Promise<components['schemas']['ReadResult']>;
  typing(id: string, isTyping: boolean): Promise<void>;
  status(id: string): Promise<components['schemas']['UserStatus']>;
}

export type PendingMessage = {
  conversation_id: string;
  client_message_id: string;
  body: string;
  status: 'sending' | 'saved' | 'failed';
  created_at: string;
};
