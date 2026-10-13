import type { QueryClient } from '@tanstack/svelte-query';
import { keys } from './cache';
import type { ChatApi, Conversation, Message, Event } from './models';
import { sequence as seq } from './models';

export type ConversationStream = {
  generation: number;
  abort: AbortController;
  applied: string;
  target: bigint;
  ready: boolean;
  loading: boolean;
  olderLoading: boolean;
  nextBefore: string | null;
  draining?: Promise<void>;
  lastRead: bigint;
  reading?: string;
};

type StoreOptions = {
  api: ChatApi;
  queryClient: QueryClient;
  userId: string;
  active: () => boolean;
  epoch: () => number;
  selectedId: () => string | null;
  changed: () => void;
  failed: (message: string) => void;
  messagesMerged: (messages: Message[]) => void;
  peerSelected: (userId: string) => void;
};

/** Owns cache mutations and continuous durable replay for one authenticated lifetime.
 * HTTP aborts save work; epoch/generation checks are the correctness boundary.
 */
export class ConversationStore {
  private streams = new Map<string, ConversationStream>();
  private summaryFlight: Promise<void> | undefined;
  private summaryAgain = false;

  constructor(private options: StoreOptions) {}

  get conversations(): Conversation[] {
    return (
      this.options.queryClient.getQueryData<Conversation[]>(
        keys.conversations,
      ) ?? []
    );
  }

  messages(id: string): Message[] {
    return (
      this.options.queryClient.getQueryData<Message[]>(keys.messages(id)) ?? []
    );
  }

  private get current() {
    return this.conversations.find(
      (conversation) => conversation.id === this.options.selectedId(),
    );
  }

  invalidate(id: string) {
    const streamState = this.stream(id);
    streamState.abort.abort();
    streamState.generation++;
    streamState.draining = undefined;
    streamState.loading = false;
    streamState.olderLoading = false;
  }

  raiseTarget(id: string, sequence: string) {
    const streamState = this.stream(id);
    streamState.target =
      streamState.target > seq(sequence) ? streamState.target : seq(sequence);
  }

  stream(id: string) {
    let streamState = this.streams.get(id);
    if (!streamState) {
      streamState = {
        generation: 0,
        abort: new AbortController(),
        applied: '0',
        target: 0n,
        ready: false,
        loading: false,
        olderLoading: false,
        nextBefore: null,
        lastRead: 0n,
      };
      this.streams.set(id, streamState);
    }
    return streamState;
  }

  private valid(
    streamState: ConversationStream,
    generation: number,
    epoch: number,
  ) {
    return (
      this.options.active() &&
      this.options.epoch() === epoch &&
      streamState.generation === generation
    );
  }

  async refreshSummaries() {
    if (!this.options.active()) {
      return;
    }
    // Coalesce concurrent hints, then rerun once so a newer notification received
    // during pagination is not lost behind the in-flight response.
    if (this.summaryFlight) {
      this.summaryAgain = true;
      return this.summaryFlight;
    }
    const epoch = this.options.epoch();
    const work = (async () => {
      do {
        this.summaryAgain = false;
        const items = new Map<string, Conversation>();
        let cursor: string | undefined;
        const seen = new Set<string>();
        do {
          const page = await this.options.api.list(cursor);
          if (!this.options.active() || epoch !== this.options.epoch()) {
            return;
          }
          for (const item of page.items) {
            items.set(item.id, item);
          }
          cursor = page.next_cursor ?? undefined;
          if (cursor) {
            if (seen.has(cursor)) {
              throw new Error('会话分页未能继续，请重试');
            }
            seen.add(cursor);
          }
        } while (cursor);
        if (!this.options.active() || epoch !== this.options.epoch()) {
          return;
        }
        this.options.queryClient.setQueryData(keys.conversations, [
          ...items.values(),
        ]);
        for (const item of items.values()) {
          const stream = this.stream(item.id);
          stream.target =
            stream.target > seq(item.latest_event_seq)
              ? stream.target
              : seq(item.latest_event_seq);
          stream.lastRead =
            stream.lastRead > seq(item.my_read_seq)
              ? stream.lastRead
              : seq(item.my_read_seq);
        }
        if (this.current) {
          void this.options.peerSelected(this.current.peer.id);
        }
      } while (this.summaryAgain && this.options.active());
    })();
    this.summaryFlight = work;

    try {
      await work;
    } catch (reason) {
      if (this.options.active() && epoch === this.options.epoch()) {
        this.options.failed(
          reason instanceof Error ? reason.message : '会话列表加载失败',
        );
      }
    } finally {
      if (this.summaryFlight === work) {
        this.summaryFlight = undefined;
      }
    }
  }

  async select(id: string, ensureReady: () => Promise<void>) {
    // Every selection gets a new generation, even when an abort cannot stop a response.
    const streamState = this.stream(id);
    streamState.abort.abort();
    streamState.abort = new AbortController();
    streamState.generation++;
    streamState.draining = undefined;
    const generation = streamState.generation;
    const epoch = this.options.epoch();
    await ensureReady();
    if (!this.valid(streamState, generation, epoch)) {
      return;
    }
    if (streamState.ready) {
      await this.drain(id);
      if (this.current) {
        void this.options.peerSelected(this.current.peer.id);
      }
      return;
    }
    streamState.loading = true;
    this.options.changed();

    try {
      const snapshot = await this.options.api.snapshot(
        id,
        undefined,
        streamState.abort.signal,
      );
      if (!this.valid(streamState, generation, epoch)) {
        return;
      }
      this.options.queryClient.setQueryData(keys.messages(id), snapshot.items);
      streamState.applied = snapshot.snapshot_seq;
      streamState.target =
        streamState.target > seq(snapshot.snapshot_seq)
          ? streamState.target
          : seq(snapshot.snapshot_seq);
      streamState.nextBefore = snapshot.next_before ?? null;
      streamState.ready = true;
      this.merge(id, snapshot.items);
      await this.drain(id);
    } catch (reason) {
      if (this.valid(streamState, generation, epoch)) {
        this.options.failed(
          reason instanceof Error ? reason.message : '消息加载失败',
        );
      }
    } finally {
      if (this.valid(streamState, generation, epoch)) {
        streamState.loading = false;
        this.options.changed();
      }
    }
  }

  async drain(id: string) {
    const streamState = this.stream(id);
    if (!streamState.ready || !this.options.active()) {
      return;
    }
    if (streamState.draining) {
      return streamState.draining;
    }
    const generation = streamState.generation;
    const epoch = this.options.epoch();
    const work = (async () => {
      while (
        this.valid(streamState, generation, epoch) &&
        seq(streamState.applied) < streamState.target
      ) {
        const page = await this.options.api.events(
          id,
          streamState.applied,
          streamState.abort.signal,
        );
        if (!this.valid(streamState, generation, epoch)) {
          return;
        }
        if (seq(page.snapshot_seq) > streamState.target) {
          streamState.target = seq(page.snapshot_seq);
        }
        // A notification only raises the target. Apply every persisted predecessor
        // before advancing the cursor, including sequences beyond Number precision.
        let progressed = false;
        for (const event of page.events) {
          if (event.envelope.conversation_id !== id) {
            throw new Error('同步响应中的会话不匹配');
          }
          const next = seq(event.envelope.event_seq);
          if (next <= seq(streamState.applied)) {
            continue;
          }
          if (next !== seq(streamState.applied) + 1n) {
            throw new Error('消息同步出现空缺，请重新加载');
          }
          this.apply(id, event);
          streamState.applied = event.envelope.event_seq;
          progressed = true;
          this.options.changed();
        }
        if (!progressed && seq(streamState.applied) < streamState.target) {
          throw new Error('消息同步暂未完成，请重新加载');
        }
      }
    })();
    streamState.draining = work;

    try {
      await work;
    } catch (reason) {
      if (this.valid(streamState, generation, epoch)) {
        this.options.failed(
          reason instanceof Error ? reason.message : '同步暂时中断',
        );
      }
    } finally {
      if (streamState.draining === work) {
        streamState.draining = undefined;
        this.options.changed();
      }
    }
  }

  private apply(id: string, event: Event) {
    const payload = event.payload;
    if (payload.type === 'message.created') {
      this.merge(id, [payload.data]);
    } else if (payload.type === 'receipt.updated') {
      const receipt = payload.data;
      const receiptField =
        receipt.user_id === this.options.userId
          ? 'my_read_seq'
          : 'peer_read_seq';
      this.options.queryClient.setQueryData<Conversation[]>(
        keys.conversations,
        (items) =>
          items?.map((item) =>
            item.id === id
              ? {
                  ...item,
                  [receiptField]:
                    seq(item[receiptField]) > seq(receipt.message_seq)
                      ? item[receiptField]
                      : receipt.message_seq,
                }
              : item,
          ),
      );
      if (receipt.user_id === this.options.userId) {
        const streamState = this.stream(id);
        streamState.lastRead =
          streamState.lastRead > seq(receipt.message_seq)
            ? streamState.lastRead
            : seq(receipt.message_seq);
      }
    }
  }

  merge(id: string, incoming: Message[]) {
    this.options.queryClient.setQueryData<Message[]>(
      keys.messages(id),
      (existing) => {
        const messages = new Map(
          (existing ?? []).map((message) => [message.id, message]),
        );
        for (const message of incoming) {
          if (message.conversation_id === id) {
            messages.set(message.id, message);
          }
        }
        return [...messages.values()].sort((left, right) =>
          seq(left.created_seq) < seq(right.created_seq)
            ? -1
            : seq(left.created_seq) > seq(right.created_seq)
              ? 1
              : 0,
        );
      },
    );
    this.options.messagesMerged(incoming);
  }

  async loadOlder(id: string) {
    const streamState = this.stream(id);
    if (!streamState.nextBefore || streamState.olderLoading) {
      return;
    }
    const generation = streamState.generation;
    const epoch = this.options.epoch();
    const before = streamState.nextBefore;
    streamState.olderLoading = true;
    this.options.changed();

    try {
      const page = await this.options.api.snapshot(
        id,
        before,
        streamState.abort.signal,
      );
      if (!this.valid(streamState, generation, epoch)) {
        return;
      }
      this.merge(id, page.items);
      streamState.nextBefore = page.next_before ?? null;
      streamState.target =
        streamState.target > seq(page.snapshot_seq)
          ? streamState.target
          : seq(page.snapshot_seq);
      await this.drain(id);
    } catch (reason) {
      if (this.valid(streamState, generation, epoch)) {
        this.options.failed(
          reason instanceof Error ? reason.message : '历史加载失败',
        );
      }
    } finally {
      if (this.valid(streamState, generation, epoch)) {
        streamState.olderLoading = false;
        this.options.changed();
      }
    }
  }

  stop() {
    for (const streamState of this.streams.values()) {
      streamState.abort.abort();
    }
    this.streams.clear();
    this.options.queryClient.clear();
  }
}
