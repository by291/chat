import type { QueryClient } from '@tanstack/svelte-query';

import { createMediaApi } from '#lib/features/media/api.ts';
import { MediaUploads } from '#lib/features/media/uploads.svelte.ts';

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
import { sequence, isGroup, membershipId } from './models';
import { HistoryNavigation } from './history.svelte';
import { MessageActions } from './message-actions.svelte';
import { ChatNotifications } from './notifications.svelte';
import {
  createPendingMessage,
  pendingMessageInput,
  unacknowledgedMessages,
} from './pending-messages';

const RECONCILIATION_INTERVAL_MS = 60_000;

export type { PendingMessage } from './models';

/** Coordinates one authenticated chat lifetime, UI drafts/sends, and rendered receipts.
 * ConversationStore owns durable state; PeerActivity owns disposable peer hints.
 */
export class ChatSync {
  readonly uploads: MediaUploads;
  readonly history: HistoryNavigation;
  readonly actions: MessageActions;
  readonly notifications: ChatNotifications;
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
  private notificationsReady = false;
  private notificationGeneration = 0;
  private epoch = 0;
  private visible = false;
  private focused = false;
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
    this.notifications = new ChatNotifications({
      userId: options.userId,
      selectedId: () => this.selectedId,
      visible: () => this.focused,
    });

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
          this.actions?.reset();
          this.history?.reset();
          this.visible = false;
          this.renderedReadSeq = null;
        }
      },
      messageApplied: (message, fresh) =>
        this.notifications.applied(
          message,
          this.conversations.find(
            (conversation) => conversation.id === message.conversation_id,
          ),
          fresh,
        ),
      messagesMerged: (incoming) => {
        this.pending = unacknowledgedMessages(
          this.pending,
          incoming,
          options.userId,
        );
      },
    });

    this.actions = new MessageActions({
      ...options,
      store: this.store,
      selectedId: () => this.selectedId,
      active: () => this.active,
      failed: (reason) => {
        this.error = reason;
      },
    });

    this.history = new HistoryNavigation({
      ...options,
      store: this.store,
      selectedId: () => this.selectedId,
      active: () => this.active,
      failed: (reason) => {
        this.error = reason;
      },
      hide: () => this.setVisible(false),
      reload: () => this.reload(),
      changed: () => {
        this.revision++;
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

  // Cache records stay authoritative. Reading this rune makes cache notifications
  // observable to Svelte getters without mirroring those records in another store.
  get conversations(): Conversation[] {
    this.revision;
    return this.store.conversations;
  }

  get messages(): Message[] {
    this.revision;
    return this.history.contextual
      ? this.history.messages
      : this.selectedId
        ? this.store.messages(this.selectedId)
        : [];
  }

  get searchResults() {
    this.revision;
    return this.history.results;
  }

  get replyTarget() {
    this.revision;
    return this.actions.replyTarget;
  }

  get canSaveEdit() {
    this.revision;
    return this.actions.canSaveEdit;
  }

  get userId() {
    return this.options.userId;
  }

  get alive() {
    return this.active;
  }

  senderName(userId: string) {
    const conversation = this.current;
    if (
      conversation &&
      !isGroup(conversation) &&
      conversation.peer.id === userId
    ) {
      return conversation.peer.display_name;
    }

    return (
      this.groupMembers.find((member) => member.user.id === userId)?.user
        .display_name ?? (userId === this.userId ? '你' : '历史成员')
    );
  }

  readCount(messageSequence: string) {
    return this.readers(messageSequence).length;
  }

  readers(messageSequence: string) {
    return this.groupMembers.filter(
      (member) =>
        member.user.id !== this.userId &&
        sequence(member.joined_seq) <= sequence(messageSequence) &&
        sequence(member.read_seq) >= sequence(messageSequence),
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
    return (
      this.history.loading ||
      (this.selectedId
        ? (this.store.stream(this.selectedId)?.loading ?? false)
        : false)
    );
  }

  get loadingOlder() {
    this.revision;
    return this.history.contextual
      ? this.history.loading
      : this.selectedId
        ? (this.store.stream(this.selectedId)?.olderLoading ?? false)
        : false;
  }

  get hasOlder() {
    this.revision;
    return this.history.contextual
      ? this.history.hasOlder
      : this.selectedId
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
            void this.restoreConnection();
          },
          (status) => {
            const previous = this.connection;
            this.connection = status;
            if (status !== 'online') {
              this.notificationsReady = false;
              this.notificationGeneration++;
              this.store.clearFresh();
            } else if (this.initialized && previous !== 'online') {
              void this.restoreConnection();
            }
          },
        );
        if (this.active) {
          this.connection = 'online';
        }
      } catch {
        // Readiness can time out while the SDK keeps reconnecting. REST polling
        // still restores durable state, so optional realtime startup is not fatal.
        if (this.active) {
          this.connection = 'offline';
        }
      }

      if (!this.active || this.epoch !== epoch) {
        return;
      }

      await this.refreshSummaries();
      if (!this.active || this.epoch !== epoch) return;
      this.store.captureNotificationBaseline();
      this.initialized = true;
      this.notificationsReady = this.connection === 'online';

      this.interval = setInterval(() => {
        void this.refresh();
      }, RECONCILIATION_INTERVAL_MS);
    })();
    return this.readiness;
  }

  async refreshSummaries() {
    await this.store.refreshSummaries();
    const id = this.selectedId;
    if (this.initialized && id && isGroup(this.current)) {
      const stream = this.store.stream(id);
      if (!stream.ready && !stream.loading) {
        await this.store.select(id, async () => {});
      }
    }
  }

  async refresh() {
    await this.refreshSummaries();
    if (this.selectedId) {
      await this.store.drain(this.selectedId);
    }
  }

  private async restoreConnection() {
    // Reconciliation fences publications queued while disconnected. After the
    // summary/replay baseline catches up, new envelopes can become candidates.
    const generation = ++this.notificationGeneration;
    this.notificationsReady = false;
    this.store.clearFresh();
    await this.refresh();
    if (
      this.active &&
      this.connection === 'online' &&
      generation === this.notificationGeneration
    ) {
      this.store.captureNotificationBaseline();
      this.notificationsReady = true;
    }
  }

  async select(id: string) {
    if (this.selectedId !== id) {
      this.history.reset();
      this.actions.reset();
    }
    if (this.selectedId && this.selectedId !== id) {
      this.store.invalidate(this.selectedId);
    }

    this.selectedId = id;
    this.visible = false;
    this.focused = false;
    this.error = '';

    await this.store.select(id, () => this.readiness ?? this.start());
  }

  async reload() {
    if (!this.selectedId) {
      return;
    }

    this.store.stream(this.selectedId).ready = false;
    this.store.stream(this.selectedId).hasSnapshot = false;
    await this.select(this.selectedId);
  }

  clearSelection() {
    this.history.reset();
    this.actions.reset();
    if (this.selectedId) {
      this.store.invalidate(this.selectedId);
      void this.setTyping(this.selectedId, false);
    }

    this.selectedId = null;
    this.visible = false;
    this.focused = false;
  }

  private receive(event: RealtimeEvent) {
    if (!this.active) {
      return;
    }
    if ('event_seq' in event) {
      this.store.liveHint(
        event.conversation_id,
        event.event_seq,
        this.notificationsReady &&
          this.connection === 'online' &&
          event.kind === 'message.created',
      );
      void this.refreshSummaries();
      return;
    }
    if (event.kind === 'conversation.preferences.updated') {
      this.store.preferencesChanged();
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

    const pending = this.preparePendingSend(
      id,
      body,
      clientMessageId,
      attachmentIds,
      membership,
    );
    if (!pending) return;

    const epoch = this.epoch;

    try {
      const message = await this.options.api.send(
        id,
        pendingMessageInput(pending),
      );
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
        // A lost response may still have committed. Keep the frozen payload and
        // client ID so an explicit retry can obtain that same authoritative record.
        const submission = this.pending.find(
          (pendingMessage) =>
            pendingMessage.conversation_id === id &&
            pendingMessage.client_message_id === clientMessageId,
        );
        if (submission) submission.status = 'failed';
      }
    }
  }

  private preparePendingSend(
    conversationId: string,
    body: string,
    clientMessageId: string,
    attachmentIds: string[],
    membershipId: string | undefined,
  ): PendingMessage | undefined {
    let pending = this.pending.find(
      (pendingMessage) =>
        pendingMessage.conversation_id === conversationId &&
        pendingMessage.client_message_id === clientMessageId,
    );
    if (pending?.status === 'sending') {
      return;
    }

    if (!pending) {
      pending = createPendingMessage({
        conversationId,
        clientMessageId,
        body,
        membershipId,
        attachmentIds,
        replyToMessageId: this.replyTarget?.id,
      });
      this.pending.push(pending);
      this.uploads.consume(attachmentIds);
      this.actions.replyId = null;
    } else {
      pending.status = 'sending';
    }

    return pending;
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
    if (this.history.contextual) {
      await this.history.older();
      return;
    }

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

  /** Page focus suppresses current-chat alerts independently of the rendered read cursor. */
  setFocused(visible: boolean) {
    this.focused = visible;
  }

  private async readLatest() {
    const id = this.selectedId;
    const cursor = this.renderedReadSeq;
    if (
      !this.active ||
      !this.visible ||
      this.history.contextual ||
      !id ||
      !cursor
    ) {
      return;
    }

    const state = this.store.stream(id);
    // A saved send can appear ahead of replay. Both rendering and contiguous
    // application are required before this cursor is safe to acknowledge.
    if (
      state.reading ||
      sequence(cursor) <= state.lastRead ||
      sequence(cursor) > sequence(state.applied)
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
          state.lastRead > sequence(result.my_read_seq)
            ? state.lastRead
            : sequence(result.my_read_seq);
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
        sequence(this.renderedReadSeq) > sequence(cursor)
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
    this.notificationsReady = false;
    this.notificationGeneration++;
    this.epoch++;
    this.visible = false;
    this.focused = false;
    if (this.interval) {
      clearInterval(this.interval);
    }

    this.activity.stop();
    this.history.reset();
    this.actions.reset();
    this.notifications.stop();
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
