import type { ChatApi, Message } from './models';
import type { ConversationStore } from './conversation-store';

/** Search/context hold IDs only; all records remain in the conversation query cache.
 * Context windows never establish a continuous latest cursor or authorize receipts.
 */
export class HistoryNavigation {
  query = $state('');
  private submittedQuery = $state('');
  searched = $state(false);
  searching = $state(false);
  loading = $state(false);
  contextual = $state(false);
  targetId = $state<string | null>(null);
  private resultIds = $state<string[]>([]);
  private contextIds = $state<string[]>([]);
  private searchBefore = $state<string | null>(null);
  private contextBefore = $state<string | null>(null);
  private generation = 0;
  private searchGeneration = 0;
  private contextGeneration = 0;
  private searchAbort = new AbortController();
  private contextAbort = new AbortController();

  constructor(
    private options: {
      api: ChatApi;
      store: ConversationStore;
      selectedId: () => string | null;
      active: () => boolean;
      failed: (reason: string) => void;
      hide: () => void;
      reload: () => Promise<void>;
      changed: () => void;
    },
  ) {}

  private records(ids: string[]) {
    const id = this.options.selectedId();
    const records = new Map(
      (id ? this.options.store.messages(id) : []).map((record) => [
        record.id,
        record,
      ]),
    );
    return ids
      .map((messageId) => records.get(messageId))
      .filter((record): record is Message => !!record);
  }

  get results() {
    return this.query.trim() === this.submittedQuery
      ? this.records(this.resultIds).filter(
          (record) =>
            !record.withdrawn && record.body.includes(this.submittedQuery),
        )
      : [];
  }

  get hasSearched() {
    return this.searched && this.query.trim() === this.submittedQuery;
  }

  get messages() {
    return this.records(this.contextIds);
  }

  get hasMoreResults() {
    return !!this.searchBefore && this.query.trim() === this.submittedQuery;
  }

  get hasOlder() {
    return !!this.contextBefore;
  }

  private valid(
    id: string,
    membership: string | undefined,
    generation: number,
  ) {
    return (
      this.options.active() &&
      this.generation === generation &&
      this.options.selectedId() === id &&
      this.options.store.membership(id) === membership
    );
  }

  async search(more = false) {
    const id = this.options.selectedId();
    const query = this.query.trim();
    if (
      !id ||
      !query ||
      [...query].length > 200 ||
      (more && (!this.hasMoreResults || this.searching))
    ) {
      return;
    }

    this.searchAbort.abort();
    this.searchAbort = new AbortController();
    const request = ++this.searchGeneration;
    const generation = this.generation;
    const membership = this.options.store.membership(id);
    this.searching = true;
    this.searched = true;
    if (!more) {
      this.resultIds = [];
      this.searchBefore = null;
      this.submittedQuery = query;
    }

    try {
      const page = await this.options.api.search(
        id,
        query,
        more ? this.searchBefore! : undefined,
        this.searchAbort.signal,
        membership,
      );
      if (
        !this.valid(id, membership, generation) ||
        request !== this.searchGeneration ||
        query !== this.query.trim() ||
        page.membership?.membership_id !== membership
      ) {
        return;
      }

      this.options.store.merge(id, page.items);
      this.resultIds = [
        ...new Set([
          ...this.resultIds,
          ...page.items.map((record) => record.id),
        ]),
      ];
      this.searchBefore = page.next_before ?? null;
      this.options.changed();
    } catch (reason) {
      if (
        this.valid(id, membership, generation) &&
        request === this.searchGeneration &&
        !this.searchAbort.signal.aborted
      ) {
        this.options.failed(
          reason instanceof Error ? reason.message : '消息搜索失败',
        );
      }
    } finally {
      if (request === this.searchGeneration) this.searching = false;
    }
  }

  async jump(messageId: string) {
    const id = this.options.selectedId();
    if (!id) return;
    this.contextAbort.abort();
    this.contextAbort = new AbortController();
    const request = ++this.contextGeneration;
    const generation = this.generation;
    const membership = this.options.store.membership(id);
    this.options.hide();
    this.contextual = true;
    this.loading = true;
    this.targetId = null;

    try {
      const page = await this.options.api.context(
        id,
        messageId,
        this.contextAbort.signal,
        membership,
      );
      if (
        !this.valid(id, membership, generation) ||
        request !== this.contextGeneration ||
        page.membership?.membership_id !== membership
      ) {
        return;
      }

      this.options.store.merge(id, page.items);
      this.contextIds = page.items.map((record) => record.id);
      this.contextBefore = page.next_before ?? null;
      this.targetId = page.target_message_id;
      this.options.changed();
    } catch (reason) {
      if (
        this.valid(id, membership, generation) &&
        request === this.contextGeneration &&
        !this.contextAbort.signal.aborted
      ) {
        this.options.failed(
          reason instanceof Error ? reason.message : '原消息不可用',
        );
      }
    } finally {
      if (request === this.contextGeneration) this.loading = false;
    }
  }

  async older() {
    const id = this.options.selectedId();
    if (!id || !this.contextBefore || this.loading) return;
    const generation = this.generation;
    const request = this.contextGeneration;
    const membership = this.options.store.membership(id);
    this.loading = true;

    try {
      const page = await this.options.api.snapshot(
        id,
        this.contextBefore,
        this.contextAbort.signal,
        membership,
      );
      if (
        !this.valid(id, membership, generation) ||
        request !== this.contextGeneration ||
        page.membership?.membership_id !== membership
      ) {
        return;
      }

      this.options.store.merge(id, page.items);
      this.contextIds = [
        ...new Set([
          ...page.items.map((record) => record.id),
          ...this.contextIds,
        ]),
      ];
      this.contextBefore = page.next_before ?? null;
      this.options.changed();
    } catch (reason) {
      if (
        this.valid(id, membership, generation) &&
        request === this.contextGeneration &&
        !this.contextAbort.signal.aborted
      ) {
        this.options.failed(
          reason instanceof Error ? reason.message : '历史加载失败',
        );
      }
    } finally {
      if (request === this.contextGeneration) this.loading = false;
    }
  }

  async latest() {
    const generation = this.generation;
    this.options.hide();
    this.contextAbort.abort();
    const request = ++this.contextGeneration;
    this.loading = true;
    await this.options.reload();
    if (
      this.generation === generation &&
      request === this.contextGeneration &&
      this.options.active()
    ) {
      this.contextual = false;
      this.contextIds = [];
      this.targetId = null;
      this.loading = false;
      this.options.changed();
    }
  }

  reset() {
    this.generation++;
    this.searchGeneration++;
    this.contextGeneration++;
    this.searchAbort.abort();
    this.contextAbort.abort();
    this.query = '';
    this.submittedQuery = '';
    this.resultIds = [];
    this.contextIds = [];
    this.searchBefore = null;
    this.contextBefore = null;
    this.targetId = null;
    this.searched = false;
    this.searching = false;
    this.loading = false;
    this.contextual = false;
  }
}
