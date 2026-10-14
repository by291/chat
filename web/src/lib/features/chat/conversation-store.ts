import type { QueryClient } from '@tanstack/svelte-query';
import { keys } from './cache';
import { ApiFailure } from '../../api-client/errors';
import { GroupRecords } from './group-records';
import { mergeMessageRecords } from './message-records';
import type { ChatApi, Conversation, Message, Event } from './models';
import { sequence as seq, isGroup, membershipId } from './models';

export type ConversationStream = {
  generation: number;
  abort: AbortController;
  applied: string;
  target: bigint;
  ready: boolean;
  hasSnapshot: boolean;
  loading: boolean;
  olderLoading: boolean;
  nextBefore: string | null;
  draining?: Promise<void>;
  lastRead: bigint;
  reading?: string;
  membershipId?: string;
  notificationFloor: bigint;
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
  membershipChanged: (id: string) => void;
  messageApplied?: (message: Message, fresh: boolean) => void;
};

/** Owns cache mutations and continuous durable replay for one authenticated lifetime.
 * HTTP aborts save work; epoch/generation checks are the correctness boundary.
 */
export class ConversationStore {
  private streams = new Map<string, ConversationStream>();
  private summaryFlight: Promise<void> | undefined;
  private summaryAgain = false;
  private summaryRevision = 0;
  private revoked = new Set<string>();
  private fresh = new Set<string>();
  readonly groups: GroupRecords;

  constructor(private options: StoreOptions) {
    this.groups = new GroupRecords({
      ...options,
      membership: (id) => this.membership(id),
    });
  }

  membership(id: string) {
    return membershipId(
      this.conversations.find((conversation) => conversation.id === id),
    );
  }

  private forget(id: string) {
    for (const candidate of this.fresh)
      if (candidate.startsWith(`${id}/`)) this.fresh.delete(candidate);
    const stream = this.streams.get(id);
    if (stream) {
      stream.abort.abort();
      stream.generation++;
    }
    this.streams.delete(id);
    this.options.queryClient.removeQueries({ queryKey: keys.messages(id) });
    this.groups.forget(id);
    this.options.membershipChanged(id);
  }

  revokeMembership(id: string, membership: string) {
    if (this.membership(id) && this.membership(id) !== membership) return false;
    this.revoked.add(`${id}/${membership}`);
    this.summaryRevision++;
    this.summaryAgain = true;
    this.forget(id);
    this.options.queryClient.setQueryData<Conversation[]>(
      keys.conversations,
      (rows) => rows?.filter((row) => row.id !== id),
    );
    return true;
  }

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
    streamState.abort = new AbortController();
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
        hasSnapshot: false,
        loading: false,
        olderLoading: false,
        nextBefore: null,
        lastRead: 0n,
        membershipId: this.membership(id),
        notificationFloor: 0n,
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
        const revision = this.summaryRevision;
        const items = new Map<string, Conversation>();
        let cursor: string | undefined;
        const seen = new Set<string>();
        do {
          const page = await this.options.api.list(cursor);
          if (!this.options.active() || epoch !== this.options.epoch()) {
            return;
          }
          for (const item of page.items) {
            if (!this.revoked.has(`${item.id}/${membershipId(item)}`))
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
        // A removal can race a paginated response. Reread rather than resurrect
        // a snapshot authorized before the control hint invalidated its episode.
        if (revision !== this.summaryRevision) {
          this.summaryAgain = true;
          continue;
        }
        for (const previous of this.conversations) {
          const incoming = items.get(previous.id);
          if (!incoming || membershipId(incoming) !== membershipId(previous))
            this.forget(previous.id);
        }
        for (const item of items.values()) {
          const previous = this.conversations.find(
            (conversation) => conversation.id === item.id,
          );
          const known =
            membershipId(previous) === membershipId(item)
              ? previous?.last_message
              : undefined;
          // Summary pages can have been read before a message mutation. Use the
          // same revision fence as the timeline so their preview cannot leak it.
          const previews = [
            known,
            this.messages(item.id).at(-1),
            item.last_message,
          ].filter((record): record is Message => !!record);
          if (previews.length)
            item.last_message = mergeMessageRecords([], previews).at(-1)!;
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
          if (this.firstFresh(item.id) !== undefined) {
            this.initializeLive(item.id);
            if (stream.ready) void this.drain(item.id);
          }
        }
        if (this.current && !isGroup(this.current)) {
          void this.options.peerSelected(this.current.peer.id);
        } else if (this.current) {
          void this.groups.load(this.current.id);
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
    // A deep link can select before startup has fetched any summary. Only an
    // uninitialized stream may adopt the now-authorized membership identity;
    // replacement of an established episode still uses forget/generation fences.
    if (!streamState.ready) streamState.membershipId = this.membership(id);
    if (streamState.ready && streamState.hasSnapshot) {
      await this.drain(id);
      if (this.current && !isGroup(this.current)) {
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
        streamState.membershipId,
      );
      if (!this.valid(streamState, generation, epoch)) {
        return;
      }
      if (snapshot.membership?.membership_id !== streamState.membershipId)
        return;
      // Snapshot responses can race a newer acknowledgement or replay revision.
      this.merge(id, snapshot.items);
      const baseline = this.liveBaseline(id, snapshot.snapshot_seq);
      if (baseline > seq(streamState.applied))
        streamState.applied = baseline.toString();
      streamState.target =
        streamState.target > seq(snapshot.snapshot_seq)
          ? streamState.target
          : seq(snapshot.snapshot_seq);
      streamState.nextBefore = snapshot.next_before ?? null;
      streamState.ready = true;
      streamState.hasSnapshot = true;
      void this.groups.load(id, streamState.abort.signal);
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
          streamState.membershipId,
        );
        if (!this.valid(streamState, generation, epoch)) {
          return;
        }
        if (page.membership?.membership_id !== streamState.membershipId) return;
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
        if (
          reason instanceof ApiFailure &&
          reason.status === 409 &&
          streamState.membershipId
        ) {
          this.forget(id);
          await this.refreshSummaries();
          if (this.options.selectedId() === id && this.membership(id))
            await this.select(id, async () => {});
          return;
        }
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
    if (
      payload.type === 'message.created' ||
      payload.type === 'message.edited' ||
      payload.type === 'message.withdrawn'
    ) {
      this.merge(id, [payload.data]);
      if (payload.type === 'message.created') {
        const fresh = this.fresh.delete(`${id}/${event.envelope.event_seq}`);
        this.options.messageApplied?.(
          this.messages(id).find((message) => message.id === payload.data.id)!,
          fresh,
        );
      }
    } else if (payload.type === 'receipt.updated') {
      const receipt = payload.data;
      if (receipt.membership_id)
        this.groups.receipt(id, receipt.membership_id, receipt.message_seq);
      const receiptField =
        receipt.user_id === this.options.userId
          ? 'my_read_seq'
          : 'peer_read_seq';
      this.options.queryClient.setQueryData<Conversation[]>(
        keys.conversations,
        (items) =>
          items?.map((item) =>
            item.id === id &&
            (!isGroup(item) || receipt.user_id === this.options.userId)
              ? {
                  ...item,
                  [receiptField]:
                    seq(
                      receiptField === 'my_read_seq'
                        ? item.my_read_seq
                        : !isGroup(item)
                          ? item.peer_read_seq
                          : '0',
                    ) > seq(receipt.message_seq)
                      ? receiptField === 'my_read_seq'
                        ? item.my_read_seq
                        : !isGroup(item)
                          ? item.peer_read_seq
                          : '0'
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
    } else if (payload.type === 'membership.updated') {
      void this.groups.load(id);
    }
  }

  merge(id: string, incoming: Message[]) {
    this.options.queryClient.setQueryData<Message[]>(
      keys.messages(id),
      (existing) =>
        mergeMessageRecords(
          existing ?? [],
          incoming.filter((message) => message.conversation_id === id),
        ),
    );
    this.options.messagesMerged(incoming);
    this.options.queryClient.setQueryData<Conversation[]>(
      keys.conversations,
      (items) =>
        items?.map((item) => {
          if (item.id !== id || !item.last_message) return item;
          const current = this.messages(id).find(
            (message) => message.id === item.last_message!.id,
          );
          return current
            ? {
                ...item,
                last_message: mergeMessageRecords(
                  [item.last_message],
                  [current],
                )[0],
              }
            : item;
        }),
    );
  }

  /** Live publications are exact candidates, never a license to notify replay backlog.
   * Retain unknown candidates until metadata authorizes their episode. Replay
   * starts before the earliest candidate even if that summary already includes it.
   * Selection still loads a latest snapshot before rendering/acknowledging history.
   */
  liveHint(id: string, eventSeq: string, notify: boolean) {
    const state = this.stream(id);
    if (
      notify &&
      seq(eventSeq) > state.notificationFloor &&
      seq(eventSeq) <= seq(state.applied)
    ) {
      const conversation = this.conversations.find(
        (record) => record.id === id,
      );
      const message = this.messages(id).find(
        (record) => record.created_seq === eventSeq,
      );
      // A preceding receipt can replay this message before its own publication.
      // The exact live hint may announce its authorized canonical record once;
      // initial/reconnect history and a cursor already read elsewhere stay silent.
      if (
        message &&
        conversation &&
        seq(conversation.my_read_seq) < seq(message.created_seq) &&
        state.lastRead < seq(message.created_seq)
      ) {
        this.options.messageApplied?.(message, true);
      }
    }
    if (
      notify &&
      seq(eventSeq) > seq(state.applied) &&
      seq(eventSeq) > state.notificationFloor
    ) {
      this.fresh.add(`${id}/${eventSeq}`);
      if (this.fresh.size > 200)
        this.fresh.delete(this.fresh.values().next().value!);
    }
    this.raiseTarget(id, eventSeq);
    this.initializeLive(id);
    if (state.ready) void this.drain(id);
  }

  private firstFresh(id: string) {
    const conversation = this.conversations.find((record) => record.id === id);
    const joined = isGroup(conversation)
      ? seq(conversation.membership.joined_seq)
      : 0n;
    const state = this.stream(id);
    let first: bigint | undefined;
    for (const candidate of this.fresh) {
      if (!candidate.startsWith(`${id}/`)) continue;
      const sequence = seq(candidate.slice(id.length + 1));
      if (sequence < joined || sequence <= state.notificationFloor) {
        this.fresh.delete(candidate);
        continue;
      }
      if (first === undefined || sequence < first) first = sequence;
    }
    return first;
  }

  private liveBaseline(id: string, snapshotSequence: string) {
    const first = this.firstFresh(id);
    return first !== undefined && first <= seq(snapshotSequence)
      ? first - 1n
      : seq(snapshotSequence);
  }

  private initializeLive(id: string) {
    const conversation = this.conversations.find((record) => record.id === id);
    const state = this.stream(id);
    if (!conversation || state.ready || state.loading) return;
    const first = this.firstFresh(id);
    // Metadata received after a creation hint may already contain the first
    // message. Await its exact live candidate instead of adopting that summary
    // as applied history. Startup/reconnect floors distinguish old outbox hints.
    if (first === undefined && state.notificationFloor === 0n) return;

    // Only an uninitialized stream adopts an episode. Established episodes are
    // replaced by forget(), so old HTTP generations cannot cross a rejoin.
    state.membershipId = membershipId(conversation);
    state.applied = this.liveBaseline(
      id,
      conversation.latest_event_seq,
    ).toString();
    state.ready = true;
  }

  /** Freeze known history after startup/reconnect, not after live metadata refreshes. */
  captureNotificationBaseline() {
    this.fresh.clear();
    for (const conversation of this.conversations) {
      this.stream(conversation.id).notificationFloor = seq(
        conversation.latest_event_seq,
      );
    }
  }

  clearFresh() {
    this.fresh.clear();
  }

  /** Reread after a preference mutation/hint fences a list authorized before it. */
  preferencesChanged() {
    this.summaryRevision++;
    void this.refreshSummaries();
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
        streamState.membershipId,
      );
      if (!this.valid(streamState, generation, epoch)) {
        return;
      }
      if (page.membership?.membership_id !== streamState.membershipId) return;
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
    this.revoked.clear();
    this.fresh.clear();
    this.options.queryClient.clear();
  }
}
