import { QueryClient } from '@tanstack/svelte-query';
import { expect, test } from 'vitest';

import type { components } from '../../src/lib/api-client';
import { SettingsSessions } from '../../src/lib/features/settings/sessions/sessions.svelte';

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

test('successful revocation refreshes authoritative sessions before clearing pending feedback', async () => {
  const queryClient = new QueryClient();
  const refreshed = deferred<Session[]>();
  let reads = 0;
  const owner = new SettingsSessions({
    queryClient,
    userId: 'alice',
    api: {
      sessions: () =>
        ++reads === 1 ? Promise.resolve([session]) : refreshed.promise,
      revokeSession: async () => undefined,
    },
  });
  await owner.load();

  const revoking = owner.revoke(session.id);
  await Promise.resolve();
  expect(owner.pending).toBe(session.id);
  expect(owner.sessions).toEqual([session]);

  refreshed.resolve([]);
  await revoking;

  expect(owner.sessions).toEqual([]);
  expect(owner.pending).toBeNull();
  expect(owner.error).toBe('');
  owner.stop();
  queryClient.clear();
});

test('stopping an old owner twice leaves a same-user successor and unrelated queries intact', async () => {
  const queryClient = new QueryClient();
  const options = {
    queryClient,
    userId: 'alice',
    api: {
      sessions: async () => [session],
      revokeSession: async () => undefined,
    },
  };
  const previous = new SettingsSessions(options);
  await previous.load();
  previous.stop();
  const successor = new SettingsSessions(options);
  await successor.load();
  queryClient.setQueryData(['settings', 'sessions', 'bob'], ['other owner']);

  previous.stop();

  expect(successor.sessions).toEqual([session]);
  expect(queryClient.getQueryData(['settings', 'sessions', 'bob'])).toEqual([
    'other owner',
  ]);
  successor.stop();
  expect(queryClient.getQueryData(['settings', 'sessions', 'bob'])).toEqual([
    'other owner',
  ]);
  queryClient.clear();
});

test.each(['initial', 'cached'])(
  'a replacement %s session read fences a late response even when transport ignores abort',
  async (state) => {
    const queryClient = new QueryClient();
    const stale = deferred<Session[]>();
    const fresh = deferred<Session[]>();
    const initialStarted = deferred<undefined>();
    const replacementStarted = deferred<undefined>();
    let staleSignal: AbortSignal | undefined;
    let reads = 0;
    if (state === 'cached') {
      queryClient.setQueryData(['settings', 'sessions', 'alice'], [session]);
    }
    const owner = new SettingsSessions({
      queryClient,
      userId: 'alice',
      api: {
        sessions: (signal) => {
          if (++reads === 1) {
            staleSignal = signal;
            initialStarted.resolve(undefined);
            return stale.promise;
          }
          replacementStarted.resolve(undefined);
          return fresh.promise;
        },
        revokeSession: async () => undefined,
      },
    });
    const initial = owner.load();
    await initialStarted.promise;

    const replacement = owner.load();
    await replacementStarted.promise;
    fresh.resolve([]);
    await replacement;
    stale.resolve([session]);
    await initial;

    expect(staleSignal?.aborted).toBe(true);
    expect(owner.sessions).toEqual([]);
    expect(queryClient.getQueryData(['settings', 'sessions', 'alice'])).toEqual(
      [],
    );
    expect(owner.error).toBe('');
    owner.stop();
    queryClient.clear();
  },
);
