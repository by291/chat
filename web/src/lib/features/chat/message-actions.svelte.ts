import type { ChatApi, Message, Preferences, ReactionKind } from './models';
import type { ConversationStore } from './conversation-store';

const MAX_MESSAGE_CHARACTERS = 4_096;

/** Owns reply selection and edit drafts; durable records stay in ConversationStore. */
export class MessageActions {
  replyId = $state<string | null>(null);
  editingId = $state<string | null>(null);
  withdrawingId = $state<string | null>(null);
  editBody = $state('');
  busy = $state(false);
  reacting = $state<string[]>([]);

  private generation = 0;

  constructor(
    private options: {
      api: ChatApi;
      store: ConversationStore;
      userId: string;
      selectedId: () => string | null;
      active: () => boolean;
      failed: (reason: string) => void;
    },
  ) {}

  get replyTarget() {
    const id = this.options.selectedId();
    return id
      ? this.options.store
          .messages(id)
          .find((record) => record.id === this.replyId)
      : undefined;
  }

  get withdrawingTarget() {
    const id = this.options.selectedId();
    return id
      ? this.options.store
          .messages(id)
          .find((record) => record.id === this.withdrawingId)
      : undefined;
  }

  get editingTarget() {
    const id = this.options.selectedId();
    return id
      ? this.options.store
          .messages(id)
          .find((record) => record.id === this.editingId)
      : undefined;
  }

  get canSaveEdit() {
    const target = this.editingTarget;
    return (
      !!target &&
      !target.withdrawn &&
      target.sender_id === this.options.userId &&
      (!!this.editBody.trim() || !!target.attachments?.length) &&
      [...this.editBody].length <= MAX_MESSAGE_CHARACTERS
    );
  }

  reply(record: Message) {
    if (
      record.conversation_id === this.options.selectedId() &&
      !record.withdrawn
    ) {
      this.replyId = record.id;
    }
  }

  edit(record: Message) {
    if (
      record.sender_id !== this.options.userId ||
      record.withdrawn ||
      record.conversation_id !== this.options.selectedId()
    ) {
      return;
    }

    this.editingId = record.id;
    this.editBody = record.body;
  }

  cancelEdit() {
    this.editingId = null;
    this.editBody = '';
  }

  async saveEdit() {
    const messageId = this.editingId;
    if (!messageId || !this.canSaveEdit) return;

    const succeeded = await this.mutate((id, membership) =>
      this.options.api.edit(id, messageId, this.editBody, membership),
    );
    if (succeeded && this.editingId === messageId) this.cancelEdit();
  }

  async withdraw(record: Message) {
    if (
      record.sender_id !== this.options.userId ||
      record.withdrawn ||
      record.conversation_id !== this.options.selectedId()
    ) {
      return;
    }

    await this.mutate((id, membership) =>
      this.options.api.withdraw(id, record.id, membership),
    );
  }

  private async mutate(
    operation: (id: string, membership?: string) => Promise<Message>,
  ) {
    const id = this.options.selectedId();
    if (!id || !this.options.active() || this.busy) return false;

    const membership = this.options.store.membership(id);
    const generation = this.generation;
    this.busy = true;

    try {
      const record = await operation(id, membership);
      if (
        !this.options.active() ||
        generation !== this.generation ||
        this.options.store.membership(id) !== membership
      ) {
        return false;
      }

      this.options.store.merge(id, [record]);
      this.options.store.raiseTarget(id, record.revision_seq);
      void this.options.store.drain(id);
      void this.options.store.refreshSummaries();
      return true;
    } catch (reason) {
      if (this.options.active() && generation === this.generation) {
        this.options.failed(
          reason instanceof Error ? reason.message : '消息操作失败',
        );
      }
      return false;
    } finally {
      if (generation === this.generation) this.busy = false;
    }
  }

  reactionPending(messageId: string): boolean {
    return this.reacting.includes(messageId);
  }

  /** Serialize one choice per message and merge only within its selected authenticated episode. */
  async reaction(messageId: string, kind: ReactionKind | null): Promise<void> {
    const id = this.options.selectedId();
    if (!id || !this.options.active() || this.reactionPending(messageId)) {
      return;
    }
    const target = this.options.store
      .messages(id)
      .find((message) => message.id === messageId);
    if (!target || target.withdrawn) return;

    const membership = this.options.store.membership(id);
    const generation = this.generation;
    this.reacting = [...this.reacting, messageId];

    try {
      const record = kind
        ? await this.options.api.setReaction(id, messageId, kind, membership)
        : await this.options.api.removeReaction(id, messageId, membership);
      if (
        !this.options.active() ||
        generation !== this.generation ||
        id !== this.options.selectedId() ||
        membership !== this.options.store.membership(id)
      ) {
        return;
      }

      this.options.store.merge(id, [record]);
      this.options.store.raiseTarget(id, record.revision_seq);
      void this.options.store.drain(id);
      void this.options.store.refreshSummaries();
    } catch (reason) {
      if (
        this.options.active() &&
        generation === this.generation &&
        id === this.options.selectedId()
      ) {
        this.options.failed(
          reason instanceof Error ? reason.message : '表情回应失败，请稍后重试',
        );
      }
    } finally {
      if (generation === this.generation) {
        this.reacting = this.reacting.filter((id) => id !== messageId);
      }
    }
  }

  async preferences(patch: Partial<Preferences>) {
    const id = this.options.selectedId();
    if (!id || !this.options.active() || this.busy) return;

    const membership = this.options.store.membership(id);
    const generation = this.generation;
    this.busy = true;

    try {
      await this.options.api.preferences(id, patch, membership);
      if (
        this.options.active() &&
        generation === this.generation &&
        membership === this.options.store.membership(id)
      ) {
        this.options.store.preferencesChanged();
      }
    } catch (reason) {
      if (this.options.active() && generation === this.generation) {
        this.options.failed(
          reason instanceof Error ? reason.message : '会话设置保存失败',
        );
      }
    } finally {
      if (generation === this.generation) this.busy = false;
    }
  }

  reset() {
    this.generation++;
    this.replyId = null;
    this.withdrawingId = null;
    this.cancelEdit();
    this.busy = false;
    this.reacting = [];
  }
}
