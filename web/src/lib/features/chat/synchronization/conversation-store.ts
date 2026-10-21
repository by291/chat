import type { QueryClient } from '@tanstack/svelte-query';

import { ApiFailure } from '#lib/api-client';

import { keys } from '../cache';
import {
  applySummaryReceipt,
  reconcileSummaryPreview,
  updateSummaryPreview,
} from '../conversations/conversation-projections';
import { collectConversationSummaries } from '../conversations/conversation-summaries';
import { GroupRecords } from '../groups/group-records';
import { mergeMessageRecords } from '../messages/message-records';
import type { ChatApi, Conversation, Message, Event } from '../models';
import { sequence, isGroup, membershipId } from '../models';
import { LiveMessageCandidates } from '../notifications/live-candidates';

type ConversationStream = {
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
  private revokedMemberships = new Set<string>();
  private liveCandidates: LiveMessageCandidates;
  readonly groups: GroupRecords;

  constructor(private options: StoreOptions) {
    this.liveCandidates = new LiveMessageCandidates({
      conversations: () => this.conversations,
      messages: (id) => this.messages(id),
      applied: options.messageApplied,
    });
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
    this.liveCandidates.forget(id);

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

    this.revokedMemberships.add(`${id}/${membership}`);
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

  raiseTarget(id: string, eventSequence: string) {
    const streamState = this.stream(id);
    const target = sequence(eventSequence);
    streamState.target =
      streamState.target > target ? streamState.target : target;
  }

  /** A detached observation cannot mutate stream generations or read fences. */
  status(id: string) {
    const state = this.stream(id);
    return Object.freeze({
      applied: state.applied,
      ready: state.ready,
      loading: state.loading,
      olderLoading: state.olderLoading,
      nextBefore: state.nextBefore,
    });
  }

  private stream(id: string) {
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
    const work = this.refreshSummaryPages(epoch);

    // Every caller joins the handled promise, including error reporting and cleanup.
    // Sharing raw work would let later callers observe an unhandled rejection.
    const flight = work
      .catch((reason) => {
        if (this.options.active() && epoch === this.options.epoch()) {
          this.options.failed(
            reason instanceof Error ? reason.message : '会话列表加载失败',
          );
        }
      })
      .finally(() => {
        if (this.summaryFlight === flight) {
          this.summaryFlight = undefined;
        }
      });
    this.summaryFlight = flight;
    return flight;
  }

  private async refreshSummaryPages(epoch: number) {
    do {
      this.summaryAgain = false;
      const revision = this.summaryRevision;
      const summaries = await collectConversationSummaries({
        api: this.options.api,
        current: () => this.options.active() && epoch === this.options.epoch(),
        revoked: (conversation) =>
          this.revokedMemberships.has(
            `${conversation.id}/${membershipId(conversation)}`,
          ),
      });
      if (
        !summaries ||
        !this.options.active() ||
        epoch !== this.options.epoch()
      ) {
        return;
      }

      // A removal can race a paginated response. Reread rather than resurrect
      // a snapshot authorized before the control hint invalidated its episode.
      if (revision !== this.summaryRevision) {
        this.summaryAgain = true;
        continue;
      }

      this.reconcileSummaries(summaries);
    } while (this.summaryAgain && this.options.active());
  }

  private reconcileSummaries(summaries: Map<string, Conversation>) {
    for (const previous of this.conversations) {
      const incoming = summaries.get(previous.id);
      if (!incoming || membershipId(incoming) !== membershipId(previous)) {
        this.forget(previous.id);
      }
    }

    // Forget old episodes before consulting the message cache. Preview revision
    // fences may reuse only records authorized for the incoming episode.
    for (const incoming of summaries.values()) {
      const previous = this.conversations.find(
        (conversation) => conversation.id === incoming.id,
      );
      summaries.set(
        incoming.id,
        reconcileSummaryPreview(incoming, previous, this.messages(incoming.id)),
      );
    }

    this.options.queryClient.setQueryData(keys.conversations, [
      ...summaries.values(),
    ]);
    this.advanceSummaryTargets(summaries.values());

    const current = this.current;
    if (current && !isGroup(current)) {
      void this.options.peerSelected(current.peer.id);
    } else if (current) {
      void this.groups.load(current.id);
    }
  }

  private advanceSummaryTargets(summaries: Iterable<Conversation>) {
    for (const conversation of summaries) {
      const streamState = this.stream(conversation.id);
      const latestEventSequence = sequence(conversation.latest_event_seq);
      const ownReadSequence = sequence(conversation.my_read_seq);
      streamState.target =
        streamState.target > latestEventSequence
          ? streamState.target
          : latestEventSequence;
      streamState.lastRead =
        streamState.lastRead > ownReadSequence
          ? streamState.lastRead
          : ownReadSequence;

      if (this.liveCandidates.first(conversation.id) !== undefined) {
        this.initializeLive(conversation.id);
        if (streamState.ready) void this.drain(conversation.id);
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

    await this.loadSnapshot(id, streamState, generation, epoch);
  }

  private async loadSnapshot(
    id: string,
    streamState: ConversationStream,
    generation: number,
    epoch: number,
  ) {
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
      if (snapshot.membership?.membership_id !== streamState.membershipId) {
        return;
      }

      // Snapshot responses can race a newer acknowledgement or replay revision.
      this.merge(id, snapshot.items);
      const baseline = this.liveCandidates.baseline(id, snapshot.snapshot_seq);
      if (baseline > sequence(streamState.applied)) {
        streamState.applied = baseline.toString();
      }
      streamState.target =
        streamState.target > sequence(snapshot.snapshot_seq)
          ? streamState.target
          : sequence(snapshot.snapshot_seq);
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

  /** Reload through selection so old snapshot/replay responses lose their generation. */
  async reload(id: string, ensureReady: () => Promise<void>) {
    const state = this.stream(id);
    state.ready = false;
    state.hasSnapshot = false;
    await this.select(id, ensureReady);
  }

  /** Accept only a DOM-confirmed cursor supplied by the visible latest timeline.
   * Rendering alone is insufficient: a send acknowledgement can precede replay.
   * The store owns continuous application, membership and request-generation fences.
   */
  async readRendered(
    id: string,
    cursor: string,
  ): Promise<'ignored' | 'acknowledged' | 'stale' | 'failed'> {
    const state = this.stream(id);
    if (
      !this.options.active() ||
      state.reading ||
      sequence(cursor) <= state.lastRead ||
      sequence(cursor) > sequence(state.applied)
    ) {
      return 'ignored';
    }

    state.reading = cursor;
    const epoch = this.options.epoch();
    const generation = state.generation;
    const membership = state.membershipId;

    try {
      const result = await this.options.api.read(id, cursor, membership);
      if (
        !this.valid(state, generation, epoch) ||
        this.membership(id) !== membership
      ) {
        return 'stale';
      }

      const acknowledged = sequence(result.my_read_seq);
      state.lastRead =
        state.lastRead > acknowledged ? state.lastRead : acknowledged;
      return 'acknowledged';
    } catch {
      // Receipts retry after another visible update, never as an offline queue.
      return 'failed';
    } finally {
      state.reading = undefined;
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
    const work = this.replay(id, streamState, generation, epoch);

    // Recovery belongs to the shared flight so all joiners wait for the same
    // membership refresh and observe the same handled transport failure.
    const flight = work
      .catch(async (reason) => {
        if (!this.valid(streamState, generation, epoch)) return;

        if (
          reason instanceof ApiFailure &&
          reason.status === 409 &&
          streamState.membershipId
        ) {
          this.forget(id);
          await this.refreshSummaries();
          if (this.options.selectedId() === id && this.membership(id)) {
            await this.select(id, async () => {});
          }
          return;
        }

        this.options.failed(
          reason instanceof Error ? reason.message : '同步暂时中断',
        );
      })
      .finally(() => {
        if (streamState.draining === flight) {
          streamState.draining = undefined;
          this.options.changed();
        }
      });
    streamState.draining = flight;
    return flight;
  }

  private async replay(
    id: string,
    streamState: ConversationStream,
    generation: number,
    epoch: number,
  ) {
    while (
      this.valid(streamState, generation, epoch) &&
      sequence(streamState.applied) < streamState.target
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

      if (sequence(page.snapshot_seq) > streamState.target) {
        streamState.target = sequence(page.snapshot_seq);
      }

      // A notification only raises the target. Apply every persisted predecessor
      // before advancing the cursor, including sequences beyond Number precision.
      let progressed = false;
      for (const event of page.events) {
        if (event.envelope.conversation_id !== id) {
          throw new Error('同步响应中的会话不匹配');
        }
        const next = sequence(event.envelope.event_seq);
        if (next <= sequence(streamState.applied)) {
          continue;
        }
        if (next !== sequence(streamState.applied) + 1n) {
          throw new Error('消息同步出现空缺，请重新加载');
        }
        this.apply(id, event);
        streamState.applied = event.envelope.event_seq;
        progressed = true;
        this.options.changed();
      }
      if (!progressed && sequence(streamState.applied) < streamState.target) {
        throw new Error('消息同步暂未完成，请重新加载');
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
        const fresh = this.liveCandidates.take(id, event.envelope.event_seq);
        this.options.messageApplied?.(
          this.messages(id).find((message) => message.id === payload.data.id)!,
          fresh,
        );
      }
    } else if (payload.type === 'message.reactions_updated') {
      if (payload.data.message) this.merge(id, [payload.data.message]);
    } else if (payload.type === 'receipt.updated') {
      const receipt = payload.data;
      if (receipt.membership_id) {
        this.groups.receipt(id, receipt.membership_id, receipt.message_seq);
      }

      this.options.queryClient.setQueryData<Conversation[]>(
        keys.conversations,
        (items) =>
          items?.map((item) =>
            item.id === id
              ? applySummaryReceipt(
                  item,
                  receipt.user_id,
                  receipt.message_seq,
                  this.options.userId,
                )
              : item,
          ),
      );

      if (receipt.user_id === this.options.userId) {
        const streamState = this.stream(id);
        streamState.lastRead =
          streamState.lastRead > sequence(receipt.message_seq)
            ? streamState.lastRead
            : sequence(receipt.message_seq);
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
        items?.map((item) =>
          item.id === id ? updateSummaryPreview(item, this.messages(id)) : item,
        ),
    );
  }

  /** Live publications are exact candidates, never a license to notify replay backlog.
   * Retain unknown candidates until metadata authorizes their episode. Replay
   * starts before the earliest candidate even if that summary already includes it.
   * Selection still loads a latest snapshot before rendering/acknowledging history.
   */
  liveHint(id: string, eventSeq: string, notify: boolean) {
    const state = this.stream(id);
    if (notify) {
      this.liveCandidates.hint(id, eventSeq, state.applied, state.lastRead);
    }

    this.raiseTarget(id, eventSeq);
    this.initializeLive(id);
    if (state.ready) void this.drain(id);
  }

  private initializeLive(id: string) {
    const conversation = this.conversations.find((record) => record.id === id);
    const state = this.stream(id);
    if (!conversation || state.ready || state.loading) return;
    const first = this.liveCandidates.first(id);
    // Metadata received after a creation hint may already contain the first
    // message. Await its exact live candidate instead of adopting that summary
    // as applied history. Startup/reconnect floors distinguish old outbox hints.
    if (first === undefined && this.liveCandidates.floor(id) === 0n) return;

    // Only an uninitialized stream adopts an episode. Established episodes are
    // replaced by forget(), so old HTTP generations cannot cross a rejoin.
    state.membershipId = membershipId(conversation);
    state.applied = this.liveCandidates
      .baseline(id, conversation.latest_event_seq)
      .toString();
    state.ready = true;
  }

  /** Freeze known history after startup/reconnect, not after live metadata refreshes. */
  captureNotificationBaseline() {
    this.liveCandidates.captureBaseline();
  }

  clearFresh() {
    this.liveCandidates.clear();
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
        streamState.target > sequence(page.snapshot_seq)
          ? streamState.target
          : sequence(page.snapshot_seq);
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
    this.revokedMemberships.clear();
    this.liveCandidates.stop();

    // Other features share this client. Whole-session disposal belongs to the
    // authenticated workspace; chat removes only its own canonical records.
    for (const queryKey of [keys.conversations, ['messages'], ['members']]) {
      this.options.queryClient.removeQueries({ queryKey });
    }
  }
}
