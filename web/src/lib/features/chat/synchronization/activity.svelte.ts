import type { ChatApi, RealtimeEvent } from '../models';

const TYPING_LIFETIME_MS = 5_000;
const TYPING_PUBLISH_INTERVAL_MS = 3_000;

type TypingHint = {
  user_id: string;
  expires_at: string;
};

/** Owns expiring peer hints and timers. Durable replay never depends on these hints. */
export class PeerActivity {
  typingByConversation = $state<Record<string, Record<string, TypingHint>>>({});
  onlineByUser = $state<Record<string, boolean | undefined>>({});

  private typingTimers = new Map<string, ReturnType<typeof setTimeout>>();
  private typingLastSent = new Map<string, number>();

  constructor(
    private options: {
      api: ChatApi;
      userId: string;
      active: () => boolean;
      epoch: () => number;
      membership: (id: string) => string | undefined;
    },
  ) {}

  receive(
    event: Exclude<
      RealtimeEvent,
      {
        event_seq: string;
      }
    >,
  ) {
    if (event.kind === 'presence.changed') {
      void this.refreshStatus(event.user_id);
      return;
    }
    if (event.kind !== 'typing.changed') return;
    if (event.user_id === this.options.userId) {
      return;
    }

    const id = event.conversation_id;
    const key = `${id}/${event.user_id}`;
    const prior = this.typingTimers.get(key);
    if (prior) {
      clearTimeout(prior);
    }

    if (!event.is_typing || Date.parse(event.expires_at) <= Date.now()) {
      this.clearTyping(id, event.user_id);
      this.typingTimers.delete(key);
      return;
    }

    this.typingByConversation[id] ??= {};
    this.typingByConversation[id][event.user_id] = {
      user_id: event.user_id,
      expires_at: event.expires_at,
    };
    this.typingTimers.set(
      key,
      setTimeout(
        () => {
          this.clearTyping(id, event.user_id);
          this.typingTimers.delete(key);
        },
        Math.min(
          TYPING_LIFETIME_MS,
          Math.max(0, Date.parse(event.expires_at) - Date.now()),
        ),
      ),
    );
  }

  async setTyping(id: string, isTyping: boolean) {
    const now = Date.now();
    if (
      isTyping &&
      now - (this.typingLastSent.get(id) ?? 0) < TYPING_PUBLISH_INTERVAL_MS
    ) {
      return;
    }
    this.typingLastSent.set(id, now);

    try {
      await this.options.api.typing(id, isTyping, this.options.membership(id));
    } catch {
      /* Ephemeral state is allowed to expire. */
    }
  }

  private clearTyping(id: string, userId: string) {
    if (!this.typingByConversation[id]) return;
    delete this.typingByConversation[id][userId];
    if (!Object.keys(this.typingByConversation[id]).length) {
      delete this.typingByConversation[id];
    }
  }

  forget(id: string) {
    for (const [key, timer] of this.typingTimers) {
      if (key.startsWith(`${id}/`)) {
        clearTimeout(timer);
        this.typingTimers.delete(key);
      }
    }
    delete this.typingByConversation[id];
    this.typingLastSent.delete(id);
  }

  async refreshStatus(userId: string) {
    const epoch = this.options.epoch();

    try {
      const status = await this.options.api.status(userId);
      if (this.options.active() && this.options.epoch() === epoch) {
        this.onlineByUser[userId] = status.online;
      }
    } catch {
      if (this.options.active() && this.options.epoch() === epoch) {
        this.onlineByUser[userId] = undefined;
      }
    }
  }

  stop() {
    for (const timer of this.typingTimers.values()) {
      clearTimeout(timer);
    }
    this.typingTimers.clear();
    this.typingLastSent.clear();
    this.typingByConversation = {};
    this.onlineByUser = {};
  }
}
