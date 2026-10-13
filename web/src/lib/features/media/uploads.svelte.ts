import type { QueryClient } from '@tanstack/svelte-query';

import { uploadObject } from '../../api-client/object-transfer';
import type { Attachment, MediaApi, UploadScope } from './models';

const MAX_FILE_BYTES = 52_428_800;
const MAX_FILES = 10;
const POLL_DEADLINE_MS = 120_000;
const attachmentKey = (id: string) => ['media', id] as const;

export type UploadDraft = {
  localId: string;
  file: File;
  scope: UploadScope;
  attachmentId?: string;
  progress: number;
  working: boolean;
  error: string;
};

function pause(signal: AbortSignal) {
  return new Promise<void>((resolve, reject) => {
    const abort = () => {
      clearTimeout(timer);
      reject(new DOMException('Canceled', 'AbortError'));
    };
    const timer = setTimeout(() => {
      signal.removeEventListener('abort', abort);
      resolve();
    }, 500);
    signal.addEventListener('abort', abort, { once: true });
    if (signal.aborted) abort();
  });
}

/** Owns files and transfer lifetimes for one session; server metadata belongs to the query cache. */
export class MediaUploads {
  entries = $state<UploadDraft[]>([]);
  error = $state('');
  private revision = $state(0);
  private active = true;
  private controllers = new Map<string, AbortController>();
  private unsubscribe: () => void;

  constructor(
    private options: {
      queryClient: QueryClient;
      api: MediaApi;
      authorized: (scope: UploadScope) => boolean;
      transfer?: typeof uploadObject;
    },
  ) {
    this.unsubscribe = options.queryClient.getQueryCache().subscribe(() => {
      this.revision++;
    });
  }

  metadata(entry: UploadDraft): Attachment | undefined {
    this.revision;
    return entry.attachmentId
      ? this.options.queryClient.getQueryData(attachmentKey(entry.attachmentId))
      : undefined;
  }

  forConversation(id: string) {
    return this.entries.filter((entry) => entry.scope.conversationId === id);
  }

  readyIds(id: string) {
    return this.forConversation(id)
      .filter(
        (entry) =>
          !entry.working &&
          !entry.error &&
          this.metadata(entry)?.state === 'ready',
      )
      .map((entry) => entry.attachmentId!);
  }

  async add(files: File[], scope: UploadScope) {
    this.error = '';
    if (!this.active || !this.options.authorized(scope)) return;
    if (files.length + this.entries.length > MAX_FILES) {
      this.error = '一次最多选择十个附件';
      return;
    }
    if (files.some((file) => !file.size || file.size > MAX_FILE_BYTES)) {
      this.error = '每个附件需要 1–50 MiB';
      return;
    }

    for (const file of files) {
      // A previous file may finish after logout/removal. Fence the whole batch,
      // not just that transfer's completion, before allocating another draft.
      if (!this.active || !this.options.authorized(scope)) return;
      const localId = crypto.randomUUID();
      this.entries.push({
        localId,
        file,
        scope: { ...scope },
        progress: 0,
        working: false,
        error: '',
      });
      const entry = this.entries.find((item) => item.localId === localId)!;
      await this.run(entry, false);
    }
  }

  private valid(entry: UploadDraft, controller: AbortController) {
    return (
      this.active &&
      !controller.signal.aborted &&
      this.controllers.get(entry.localId) === controller &&
      this.entries.includes(entry) &&
      this.options.authorized(entry.scope)
    );
  }

  private save(entry: UploadDraft, attachment: Attachment) {
    entry.attachmentId = attachment.id;
    this.options.queryClient.setQueryData(
      attachmentKey(attachment.id),
      attachment,
    );
  }

  private async poll(entry: UploadDraft, controller: AbortController) {
    const deadline = Date.now() + POLL_DEADLINE_MS;
    while (this.valid(entry, controller)) {
      const record = this.metadata(entry);
      if (record?.state === 'ready') return;
      if (record?.state === 'failed' || record?.state === 'expired')
        throw new Error('文件处理失败，请重新选择文件');
      if (Date.now() >= deadline) throw new Error('文件仍在处理中，请稍后重试');
      await pause(controller.signal);
      const next = await this.options.api.status(
        entry.attachmentId!,
        controller.signal,
      );
      if (this.valid(entry, controller)) this.save(entry, next);
    }
  }

  private async run(entry: UploadDraft, retry: boolean) {
    if (!this.active || !this.options.authorized(entry.scope)) return;
    this.controllers.get(entry.localId)?.abort();
    const controller = new AbortController();
    this.controllers.set(entry.localId, controller);
    entry.working = true;
    entry.error = '';

    try {
      if (retry && entry.attachmentId) {
        // Completion can succeed after its HTTP response is lost. Inspect durable
        // state before attempting a renewal that would create a different PUT.
        const record = await this.options.api.status(
          entry.attachmentId,
          controller.signal,
        );
        if (!this.valid(entry, controller)) return;
        this.save(entry, record);
        if (record.state !== 'uploading') {
          await this.poll(entry, controller);
          return;
        }
      }
      const grant = entry.attachmentId
        ? await this.options.api.renew(entry.attachmentId, controller.signal)
        : await this.options.api.create(
            {
              purpose: entry.scope.purpose,
              conversation_id: entry.scope.conversationId ?? null,
              membership_id: entry.scope.membershipId ?? null,
              filename: entry.file.name,
              content_type: entry.file.type || 'application/octet-stream',
              size_bytes: entry.file.size,
            },
            controller.signal,
          );
      if (!this.valid(entry, controller)) return;
      this.save(entry, grant.attachment);
      entry.progress = 0;
      await (this.options.transfer ?? uploadObject)(grant, entry.file, {
        signal: controller.signal,
        progress: (percent) => {
          if (this.valid(entry, controller)) entry.progress = percent;
        },
      });
      if (!this.valid(entry, controller)) return;
      const record = await this.options.api.complete(
        grant.attachment.id,
        grant.attempt_id,
        controller.signal,
      );
      if (!this.valid(entry, controller)) return;
      this.save(entry, record);
      await this.poll(entry, controller);
    } catch (reason) {
      if (this.valid(entry, controller))
        entry.error =
          reason instanceof Error ? reason.message : '上传失败，请重试';
    } finally {
      if (this.controllers.get(entry.localId) === controller) {
        this.controllers.delete(entry.localId);
        entry.working = false;
      }
    }
  }

  async retry(localId: string) {
    const entry = this.entries.find((item) => item.localId === localId);
    if (entry && !entry.working) await this.run(entry, true);
  }

  async cancel(localId: string) {
    const entry = this.entries.find((item) => item.localId === localId);
    if (!entry) return;
    this.controllers.get(localId)?.abort();
    try {
      if (entry.attachmentId) await this.options.api.cancel(entry.attachmentId);
      this.discard(entry);
    } catch (reason) {
      if (this.active && this.entries.includes(entry))
        entry.error =
          reason instanceof Error ? reason.message : '取消失败，请重试';
    }
  }

  private discard(entry: UploadDraft) {
    this.controllers.get(entry.localId)?.abort();
    this.controllers.delete(entry.localId);
    this.entries = this.entries.filter(
      (item) => item.localId !== entry.localId,
    );
  }

  consume(ids: string[]) {
    for (const entry of this.entries)
      if (entry.attachmentId && ids.includes(entry.attachmentId))
        this.discard(entry);
  }

  forgetConversation(id: string) {
    // Revoked uploads cannot be canceled through an ended membership. The server's
    // expiry/cleanup ledger reclaims these reservations after outstanding PUT grants.
    for (const entry of this.forConversation(id)) this.discard(entry);
  }

  stop() {
    this.active = false;
    for (const controller of this.controllers.values()) controller.abort();
    this.controllers.clear();
    this.entries = [];
    this.options.queryClient.removeQueries({ queryKey: ['media'] });
    this.unsubscribe();
  }
}
