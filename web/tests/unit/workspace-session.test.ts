import { afterEach, expect, test, vi } from 'vitest';
import { QueryClient } from '@tanstack/svelte-query';

import { startWorkspaceSession } from '../../src/lib/features/workspace/session';

const fixture = vi.hoisted(() => ({
  user: { id: 'alice' } as { id: string } | undefined,
  resetHandlers: new Set<() => void>(),
  stopped: [] as string[],
}));

vi.mock('#lib/features/auth/session.svelte.ts', () => ({
  auth: {
    get user() {
      return fixture.user;
    },
    onReset(callback: () => void) {
      fixture.resetHandlers.add(callback);
      return () => fixture.resetHandlers.delete(callback);
    },
    reset() {
      fixture.user = undefined;
      for (const callback of fixture.resetHandlers) callback();
    },
  },
}));

vi.mock('#lib/features/chat/sync.svelte.ts', () => ({
  createChatSync: ({ queryClient }: { queryClient: QueryClient }) => ({
    start: async () => {},
    stop: () => {
      fixture.stopped.push('chat');
      // An owner may publish its final state while stopping. Whole-session
      // cache clearing must happen after that final synchronous publication.
      queryClient.setQueryData(['chat-final-state'], 'stopped');
    },
  }),
}));

vi.mock('#lib/features/chat/synchronization/realtime.ts', () => ({
  createRealtimeTransport: () => ({}),
}));

vi.mock('#lib/features/contacts/contacts.svelte.ts', () => ({
  ContactsState: class {
    async start() {}
    async refresh() {}
    stop() {
      fixture.stopped.push('contacts');
    }
  },
}));

vi.mock('#lib/features/notifications/subscriptions.svelte.ts', () => ({
  PushSubscriptions: class {
    async start() {}
    async stop() {
      fixture.stopped.push('push-revoked');
    }
    dispose() {
      fixture.stopped.push('push-disposed');
    }
  },
}));

afterEach(() => {
  fixture.user = { id: 'alice' };
  fixture.resetHandlers.clear();
  fixture.stopped.length = 0;
});

function workspace() {
  const queryClient = new QueryClient();
  const options: Parameters<typeof startWorkspaceSession>[0] = {
    userId: 'alice',
    queryClient,
    chat: {
      sync: null,
      listView: { filter: 'saved', archived: true, scrollTop: 80 },
      readingPositions: new Map(),
    },
    contacts: {
      owner: null,
      view: { category: 'requests', selectedId: 'request', scrollTop: 40 },
    },
    backgroundPush: { owner: null },
  };
  queryClient.setQueryData(['another-feature', 'private-record'], 'secret');

  return { ...options, stop: startWorkspaceSession(options) };
}

test('workspace teardown clears every feature cache after stopping its owners', () => {
  const owner = workspace();

  fixture.user = undefined;
  owner.stop();

  expect(fixture.stopped).toEqual(['push-revoked', 'contacts', 'chat']);
  expect(owner.queryClient.getQueryCache().getAll()).toEqual([]);
  expect(owner.chat.sync).toBeNull();
  expect(owner.contacts.owner).toBeNull();
  expect(owner.backgroundPush.owner).toBeNull();
  expect(fixture.resetHandlers.size).toBe(0);
});

test('leaving the workspace with the same identity preserves browser capability', () => {
  const owner = workspace();

  // A refreshed profile object is still the same authenticated identity.
  fixture.user = { id: 'alice' };
  owner.stop();

  expect(fixture.stopped).toEqual(['push-disposed', 'contacts', 'chat']);
  expect(owner.queryClient.getQueryCache().getAll()).toEqual([]);
  expect(owner.contacts.view).toEqual({
    category: 'friends',
    selectedId: null,
    scrollTop: 0,
  });
});

test('session reset revokes push immediately before effect disposal can run', () => {
  const owner = workspace();

  fixture.user = undefined;
  for (const callback of fixture.resetHandlers) callback();

  expect(fixture.stopped).toEqual(['push-revoked']);

  owner.stop();
  expect(owner.queryClient.getQueryCache().getAll()).toEqual([]);
  expect(fixture.resetHandlers.size).toBe(0);
});
