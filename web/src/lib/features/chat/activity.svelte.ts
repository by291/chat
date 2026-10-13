import type { ChatApi, RealtimeEvent } from './models';

/** Owns expiring peer hints and timers. Durable replay never depends on these hints. */
export class PeerActivity {
  typingByConversation = $state<
    Record<
      string,
      {
        user_id: string;
        expires_at: string;
      }
    >
  >({});
  onlineByUser = $state<Record<string, boolean | undefined>>({});
  private typingTimers = new Map<string, ReturnType<typeof setTimeout>>();
  private typingLastSent = new Map<string, number>();

  constructor(
    private options: {
      api: ChatApi;
      userId: string;
      active: () => boolean;
      epoch: () => number;
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
    if (event.user_id === this.options.userId) {
      return;
    }
    const id = event.conversation_id;
    const prior = this.typingTimers.get(id);
    if (prior) {
      clearTimeout(prior);
    }
    if (!event.is_typing || Date.parse(event.expires_at) <= Date.now()) {
      delete this.typingByConversation[id];
      return;
    }
    this.typingByConversation[id] = {
      user_id: event.user_id,
      expires_at: event.expires_at,
    };
    this.typingTimers.set(
      id,
      setTimeout(
        () => {
          delete this.typingByConversation[id];
          this.typingTimers.delete(id);
        },
        Math.min(5000, Math.max(0, Date.parse(event.expires_at) - Date.now())),
      ),
    );
  }

  async setTyping(id: string, isTyping: boolean) {
    const now = Date.now();
    if (isTyping && now - (this.typingLastSent.get(id) ?? 0) < 3000) {
      return;
    }
    this.typingLastSent.set(id, now);

    try {
      await this.options.api.typing(id, isTyping);
    } catch {
      /* Ephemeral state is allowed to expire. */
    }
  }

  async refreshStatus(user: string) {
    const epoch = this.options.epoch();

    try {
      const status = await this.options.api.status(user);
      if (this.options.active() && this.options.epoch() === epoch) {
        this.onlineByUser[user] = status.online;
      }
    } catch {
      if (this.options.active() && this.options.epoch() === epoch) {
        this.onlineByUser[user] = undefined;
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
