import { afterEach, expect, test } from 'vitest';
import { QueryClient } from '@tanstack/svelte-query';
import type { uploadObject } from '../../src/lib/api-client/object-transfer';

import { MediaUploads } from '../../src/lib/features/media/uploads.svelte';
import type {
  Attachment,
  MediaApi,
  UploadGrant,
} from '../../src/lib/features/media/models';

const controllers: MediaUploads[] = [];
afterEach(() => {
  for (const controller of controllers) controller.stop();
  controllers.length = 0;
});

function attachment(): Attachment {
  return {
    id: 'attachment',
    purpose: 'message',
    state: 'ready',
    filename: 'note.txt',
    content_type: 'application/octet-stream',
    size_bytes: 4,
    width: null,
    height: null,
    sha256: 'hash',
    error_code: null,
  };
}

function grant(attempt = 'first'): UploadGrant {
  return {
    attachment: { ...attachment(), state: 'uploading' },
    attempt_id: attempt,
    url: 'https://media.invalid/object',
    headers: {},
    expires_at: '2026-10-13T00:00:00Z',
  };
}

function fixture(
  overrides: Partial<MediaApi> = {},
  transfer: typeof uploadObject = async () => {},
) {
  let membership = 'old';
  const requests: MediaApi = {
    create: async () => grant(),
    renew: async () => grant('second'),
    complete: async () => attachment(),
    status: async () => attachment(),
    cancel: async () => {},
    download: async () => {
      throw new Error('unused');
    },
    ...overrides,
  };
  const uploads = new MediaUploads({
    queryClient: new QueryClient(),
    api: requests,
    transfer,
    authorized: (scope) =>
      !scope.membershipId || scope.membershipId === membership,
  });
  controllers.push(uploads);
  return {
    uploads,
    rejoin: () => {
      membership = 'new';
      uploads.forgetConversation('group');
    },
  };
}

test('retry renews the staging attempt and completes only the newly transferred attempt', async () => {
  const attempts: string[] = [];
  let transfers = 0;
  const { uploads } = fixture(
    {
      status: async () => ({ ...attachment(), state: 'uploading' }),
      complete: async (_id, attempt) => {
        attempts.push(attempt);
        return attachment();
      },
    },
    async () => {
      if (++transfers === 1) throw new Error('connection lost');
    },
  );
  await uploads.add([new File(['note'], 'note.txt')], {
    purpose: 'message',
    conversationId: 'group',
    membershipId: 'old',
  });
  const entry = uploads.entries[0];
  expect(entry.error).toBe('connection lost');

  await uploads.retry(entry.localId);

  expect(attempts).toEqual(['second']);
  expect(uploads.readyIds('group')).toEqual(['attachment']);
});

test('a late completion from a removed episode cannot restore its upload draft', async () => {
  let finish!: (value: Attachment) => void;
  let completing = false;
  const response = new Promise<Attachment>((resolve) => {
    finish = resolve;
  });
  const { uploads, rejoin } = fixture({
    complete: async () => {
      completing = true;
      return response;
    },
  });
  const adding = uploads.add(
    [new File(['note'], 'note.txt'), new File(['next'], 'next.txt')],
    {
      purpose: 'message',
      conversationId: 'group',
      membershipId: 'old',
    },
  );
  await expect.poll(() => completing).toBe(true);

  rejoin();
  finish(attachment());
  await adding;

  expect(uploads.entries).toEqual([]);
  expect(uploads.readyIds('group')).toEqual([]);
});

test('logout aborts transfers, clears files and prevents late server results', async () => {
  let signal!: AbortSignal;
  const { uploads } = fixture({}, async (_grant, _file, options) => {
    signal = options.signal;
    await new Promise<void>((resolve) =>
      options.signal.addEventListener('abort', () => resolve(), { once: true }),
    );
  });
  const adding = uploads.add([new File(['note'], 'note.txt')], {
    purpose: 'message',
    conversationId: 'group',
  });
  await expect.poll(() => Boolean(signal)).toBe(true);

  uploads.stop();
  await adding;

  expect(signal.aborted).toBe(true);
  expect(uploads.entries).toEqual([]);
});
