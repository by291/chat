import type { components } from '../../api-client';

/** Database bigint sequences arrive as canonical strings; Number would lose bits. */
export const sequence = (value: string) => BigInt(value);

export type Conversation = components['schemas']['ConversationSummary'];
export type Group = components['schemas']['GroupConversationView'];
export type GroupMember = components['schemas']['GroupMemberView'];

export function isGroup(
  conversation: Conversation | undefined,
): conversation is Group {
  return (
    !!conversation && 'kind' in conversation && conversation.kind === 'group'
  );
}

export function conversationName(
  conversation: Conversation | undefined,
): string {
  return isGroup(conversation)
    ? conversation.title
    : (conversation?.peer.display_name ?? '聊天');
}

export function membershipId(
  conversation: Conversation | undefined,
): string | undefined {
  return isGroup(conversation)
    ? conversation.membership.membership_id
    : undefined;
}

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
  ): Promise<components['schemas']['Page_ConversationSummary']>;
  conversation(id: string, signal?: AbortSignal): Promise<Conversation>;
  snapshot(
    id: string,
    before?: string,
    signal?: AbortSignal,
    membershipId?: string,
  ): Promise<Snapshot>;
  events(
    id: string,
    after: string,
    signal?: AbortSignal,
    membershipId?: string,
  ): Promise<EventPage>;
  members(id: string, signal?: AbortSignal): Promise<GroupMember[]>;
  send(
    id: string,
    input: components['schemas']['SendMessageInput'],
  ): Promise<Message>;
  read(
    id: string,
    seq: string,
    membershipId?: string,
  ): Promise<components['schemas']['ReadResult']>;
  typing(id: string, isTyping: boolean, membershipId?: string): Promise<void>;
  status(id: string): Promise<components['schemas']['UserStatus']>;
}

export type PendingMessage = {
  conversation_id: string;
  client_message_id: string;
  body: string;
  status: 'sending' | 'saved' | 'failed';
  created_at: string;
  membership_id?: string;
  attachment_ids: string[];
};
