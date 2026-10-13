import type { QueryClient } from '@tanstack/svelte-query';
import type {
  ChatApi,
  Message,
  Conversation,
  RealtimeEvent,
  PendingMessage,
} from './models';
import type { RealtimeTransport, ConnectionStatus } from './realtime';
import { ConversationStore } from './conversation-store';
import { PeerActivity } from './activity.svelte';
import { sequence as seq, isGroup, membershipId } from './models';
import { createMediaApi } from '../media/api';
import { MediaUploads } from '../media/uploads.svelte';

export type { PendingMessage } from './models';

/** Coordinates one authenticated chat lifetime, UI drafts/sends, and rendered receipts.
 * ConversationStore owns durable state; PeerActivity owns disposable peer hints.
 */
export class ChatSync {
  readonly uploads: MediaUploads;
  selectedId = $state<string | null>(null);
  pending = $state<PendingMessage[]>([]);
  drafts = $state<Record<string, string>>({});
  connection = $state<ConnectionStatus>('connecting');
  error = $state('');
  private revision = $state(0);
  private store: ConversationStore;
  private activity: PeerActivity;
  private active = false;
  private initialized = false;
  private epoch = 0;
  private visible = false;
  private renderedReadSeq: string | null = null;
  private readiness: Promise<void> | null = null;
  private interval: ReturnType<typeof setInterval> | undefined;
  private unsubscribeCache: () => void;

  constructor(
    private options: {
      api: ChatApi;
      transport: RealtimeTransport;
      queryClient: QueryClient;
      userId: string;
    },
  ) {
    this.activity = new PeerActivity({
      ...options,
      active: () => this.active,
      epoch: () => this.epoch,
      membership: (id) => this.store?.membership(id),
    });
    this.store = new ConversationStore({
      ...options,
      active: () => this.active,
      epoch: () => this.epoch,
      selectedId: () => this.selectedId,
      changed: () => {
        this.revision++;
      },
      failed: (message) => {
        this.error = message;
      },
      peerSelected: (userId) => {
        void this.activity.refreshStatus(userId);
      },
      membershipChanged: (id) => {
        this.uploads?.forgetConversation(id);
        this.pending = this.pending.filter(
          (pending) => pending.conversation_id !== id,
        );
        delete this.drafts[id];
        this.activity.forget(id);
        if (this.selectedId === id) {
          this.visible = false;
          this.renderedReadSeq = null;
        }
      },
      messagesMerged: (incoming) => {
        // Only our sender/client-ID pair resolves an optimistic send; another
        // member can independently choose the same client-generated UUID.
        this.pending = this.pending.filter(
          (pending) =>
            !incoming.some(
              (message) =>
                message.conversation_id === pending.conversation_id &&
                message.sender_id === options.userId &&
                message.client_message_id === pending.client_message_id,
            ),
        );
      },
    });
    this.uploads = new MediaUploads({
      queryClient: options.queryClient,
      api: createMediaApi(),
      authorized: (scope) =>
        this.active &&
        (scope.purpose === 'avatar' ||
          this.conversations.some(
            (conversation) =>
              conversation.id === scope.conversationId &&
              membershipId(conversation) === scope.membershipId,
          )),
    });
    this.unsubscribeCache = options.queryClient
      .getQueryCache()
      .subscribe(() => {
        this.revision++;
      });
  }

  get conversations(): Conversation[] {
    this.revision;
    return this.store.conversations;
  }

  get messages(): Message[] {
    this.revision;
    return this.selectedId ? this.store.messages(this.selectedId) : [];
  }

  get userId() {
    return this.options.userId;
  }

  get alive() {
    return this.active;
  }

  senderName(userId: string) {
    return (
      this.groupMembers.find((member) => member.user.id === userId)?.user
        .display_name ?? (userId === this.userId ? '你' : '历史成员')
    );
  }

  readCount(sequence: string) {
    return this.readers(sequence).length;
  }

  readers(sequence: string) {
    return this.groupMembers.filter(
      (member) =>
        member.user.id !== this.userId &&
        seq(member.joined_seq) <= seq(sequence) &&
        seq(member.read_seq) >= seq(sequence),
    );
  }

  get groupMembers() {
    this.revision;
    return this.selectedId ? this.store.groups.members(this.selectedId) : [];
  }

  async refreshMembers() {
    if (this.selectedId) await this.store.groups.load(this.selectedId);
  }

  get typingByConversation() {
    return this.activity.typingByConversation;
  }

  get onlineByUser() {
    return this.activity.onlineByUser;
  }

  get current() {
    return this.conversations.find(
      (conversation) => conversation.id === this.selectedId,
    );
  }

  get appliedSeq() {
    this.revision;
    return this.selectedId
      ? (this.store.stream(this.selectedId)?.applied ?? '0')
      : '0';
  }

  get loading() {
    this.revision;
    return this.selectedId
      ? (this.store.stream(this.selectedId)?.loading ?? false)
      : false;
  }

  get loadingOlder() {
    this.revision;
    return this.selectedId
      ? (this.store.stream(this.selectedId)?.olderLoading ?? false)
      : false;
  }

  get hasOlder() {
    this.revision;
    return this.selectedId
      ? !!this.store.stream(this.selectedId)?.nextBefore
      : false;
  }

  get currentPending() {
    return this.pending.filter(
      (message) =>
        message.conversation_id === this.selectedId &&
        message.membership_id === membershipId(this.current),
    );
  }

  async start() {
    if (this.readiness) {
      return this.readiness;
    }
    this.active = true;
    const epoch = this.epoch;
    this.readiness = (async () => {
      try {
        // Subscribe before taking snapshots so notifications can raise their replay targets.
        await this.options.transport.subscribe(
          (event) => this.receive(event),
          () => {
            void this.refresh();
          },
          (status) => {
            const previous = this.connection;
            this.connection = status;
            if (status === 'online' && previous === 'offline') {
              void this.refresh();
            }
          },
        );
        if (this.active) {
          this.connection = 'online';
        }
      } catch {
        if (this.active) {
          this.connection = 'offline';
        }
      }
      if (!this.active || this.epoch !== epoch) {
        return;
      }
      await this.refreshSummaries();
      this.initialized = true;
      this.interval = setInterval(() => {
        void this.refresh();
      }, 60000);
    })();
    return this.readiness;
  }

  async refreshSummaries() {
    await this.store.refreshSummaries();
    const id = this.selectedId;
    if (this.initialized && id && isGroup(this.current)) {
      const stream = this.store.stream(id);
      if (!stream.ready && !stream.loading)
        await this.store.select(id, async () => {});
    }
  }

  async refresh() {
    await this.refreshSummaries();
    if (this.selectedId) {
      await this.store.drain(this.selectedId);
    }
  }

  async select(id: string) {
    if (this.selectedId && this.selectedId !== id) {
      this.store.invalidate(this.selectedId);
    }
    this.selectedId = id;
    this.visible = false;
    this.error = '';
    await this.store.select(id, () => this.readiness ?? this.start());
  }

  async reload() {
    if (!this.selectedId) {
      return;
    }
    this.store.stream(this.selectedId).ready = false;
    await this.select(this.selectedId);
  }

  clearSelection() {
    if (this.selectedId) {
      this.store.invalidate(this.selectedId);
      void this.setTyping(this.selectedId, false);
    }
    this.selectedId = null;
    this.visible = false;
  }

  private receive(event: RealtimeEvent) {
    if (!this.active) {
      return;
    }
    if ('event_seq' in event) {
      this.store.raiseTarget(event.conversation_id, event.event_seq);
      if (
        event.conversation_id === this.selectedId &&
        this.store.stream(event.conversation_id).ready
      ) {
        void this.store.drain(event.conversation_id);
      }
      void this.refreshSummaries();
      return;
    }
    if (event.kind === 'membership.invalidated') {
      if (
        this.store.revokeMembership(event.conversation_id, event.membership_id)
      ) {
        this.error = '你已不在此群聊中';
        void this.refreshSummaries();
      }
      return;
    }
    this.activity.receive(event);
  }

  async send(
    body: string,
    clientMessageId: string = crypto.randomUUID(),
    attachmentIds: string[] = [],
  ) {
    const id = this.selectedId;
    const membership = id ? this.store.membership(id) : undefined;
    if (!id || (!body.trim() && !attachmentIds.length) || !this.active) {
      return;
    }
    let pending = this.pending.find(
      (pendingMessage) =>
        pendingMessage.conversation_id === id &&
        pendingMessage.client_message_id === clientMessageId,
    );
    if (pending?.status === 'sending') {
      return;
    }
    if (!pending) {
      pending = {
        conversation_id: id,
        client_message_id: clientMessageId,
        body,
        status: 'sending',
        created_at: new Date().toISOString(),
        membership_id: membership,
        attachment_ids: [...attachmentIds],
      };
      this.pending.push(pending);
      this.uploads.consume(attachmentIds);
    } else {
      pending.status = 'sending';
      body = pending.body;
    }
    const epoch = this.epoch;

    try {
      const message = await this.options.api.send(id, {
        client_message_id: clientMessageId,
        body,
        membership_id: pending.membership_id,
        ...(pending.attachment_ids.length
          ? { attachment_ids: pending.attachment_ids }
          : {}),
      });
      if (
        !this.active ||
        epoch !== this.epoch ||
        this.store.membership(id) !== membership
      ) {
        return;
      }
      this.store.merge(id, [message]);
      this.store.raiseTarget(id, message.created_seq);
      void this.store.drain(id);
      void this.refreshSummaries();
    } catch {
      if (
        this.active &&
        epoch === this.epoch &&
        this.store.membership(id) === membership
      ) {
        const item = this.pending.find(
          (pendingMessage) =>
            pendingMessage.conversation_id === id &&
            pendingMessage.client_message_id === clientMessageId,
        );
        if (item) {
          item.status = 'failed';
        }
      }
    }
  }

  async retry(clientMessageId: string) {
    const pending = this.pending.find(
      (pendingMessage) =>
        pendingMessage.client_message_id === clientMessageId &&
        pendingMessage.conversation_id === this.selectedId,
    );
    if (pending) {
      await this.send(pending.body, clientMessageId, pending.attachment_ids);
    }
  }

  async loadOlder() {
    if (this.selectedId) {
      await this.store.loadOlder(this.selectedId);
    }
  }

  setVisible(visible: boolean) {
    this.visible = visible;
    // Capture only the cursor confirmed by MessageList after its DOM update.
    // A later HTTP response/replay must not silently acknowledge unrendered messages.
    this.renderedReadSeq = visible
      ? (this.messages.at(-1)?.created_seq ?? null)
      : null;
    if (visible) {
      void this.readLatest();
    }
  }

  private async readLatest() {
    const id = this.selectedId;
    const cursor = this.renderedReadSeq;
    if (!this.active || !this.visible || !id || !cursor) {
      return;
    }
    const state = this.store.stream(id);
    // A saved send can appear ahead of replay. Both rendering and contiguous
    // application are required before this cursor is safe to acknowledge.
    if (
      state.reading ||
      seq(cursor) <= state.lastRead ||
      seq(cursor) > seq(state.applied)
    ) {
      return;
    }
    state.reading = cursor;
    const epoch = this.epoch;
    const generation = state.generation;
    const membership = state.membershipId;

    try {
      const result = await this.options.api.read(id, cursor, membership);
      if (
        this.active &&
        this.epoch === epoch &&
        state.generation === generation &&
        this.store.membership(id) === membership
      ) {
        state.lastRead =
          state.lastRead > seq(result.my_read_seq)
            ? state.lastRead
            : seq(result.my_read_seq);
        void this.refreshSummaries();
      }
    } catch {
      /* Read receipts retry only after another visible update, never as an offline queue. */
    } finally {
      state.reading = undefined;
      if (
        this.active &&
        this.visible &&
        this.selectedId === id &&
        this.renderedReadSeq &&
        seq(this.renderedReadSeq) > seq(cursor)
      ) {
        void this.readLatest();
      }
    }
  }

  async setTyping(id: string, isTyping: boolean) {
    if (
      !this.active ||
      (isTyping && (!this.visible || id !== this.selectedId))
    ) {
      return;
    }
    await this.activity.setTyping(id, isTyping);
  }

  stop() {
    this.active = false;
    this.epoch++;
    this.visible = false;
    if (this.interval) {
      clearInterval(this.interval);
    }
    this.activity.stop();
    this.uploads.stop();
    this.options.transport.close();
    this.unsubscribeCache();
    this.pending = [];
    this.drafts = {};
    this.store.stop();
  }
}

export function createChatSync(
  options: ConstructorParameters<typeof ChatSync>[0],
) {
  return new ChatSync(options);
}
