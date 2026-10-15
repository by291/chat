import { QueryClient } from '@tanstack/svelte-query';
import { expect, test } from 'vitest';

import type { components } from '../../src/lib/api-client';
import { SettingsSessions } from '../../src/lib/features/settings/sessions.svelte';

type Session = components['schemas']['SessionView'];

const session: Session = {
  id: 'device',
  device_label: 'Browser',
  created_at: '2026-10-15T00:00:00Z',
  expires_at: '2026-11-15T00:00:00Z',
  is_current: false,
};

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((complete) => {
    resolve = complete;
  });

  return { promise, resolve };
}

test('a session-list response after teardown cannot repopulate the cleared authenticated cache', async () => {
  const queryClient = new QueryClient();
  const response = deferred<Session[]>();
  const owner = new SettingsSessions({
    queryClient,
    userId: 'alice',
    api: {
      sessions: () => response.promise,
      revokeSession: async () => undefined,
    },
  });

  const loading = owner.load();
  owner.stop();
  queryClient.clear();
  response.resolve([session]);
  await loading;

  expect(queryClient.getQueryCache().getAll()).toHaveLength(0);
  expect(owner.sessions).toEqual([]);
  expect(owner.error).toBe('');
});

test('a revoke response after logout cannot reload an ended session list', async () => {
  const queryClient = new QueryClient();
  const revoked = deferred<undefined>();
  const owner = new SettingsSessions({
    queryClient,
    userId: 'alice',
    api: {
      sessions: async () => [session],
      revokeSession: () => revoked.promise,
    },
  });
  await owner.load();
  expect(owner.sessions).toEqual([session]);

  const revoking = owner.revoke(session.id);
  owner.stop();
  queryClient.clear();
  revoked.resolve(undefined);
  await revoking;

  expect(queryClient.getQueryCache().getAll()).toHaveLength(0);
  expect(owner.sessions).toEqual([]);
  expect(owner.pending).toBeNull();
  expect(owner.error).toBe('');
});
