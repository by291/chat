import { afterEach, expect, test, vi } from 'vitest';

import type { components } from '../../src/lib/api-client';
import { createQueryClient, keys } from '../../src/lib/features/chat/cache';
import { ConversationStore } from '../../src/lib/features/chat/synchronization/conversation-store';
import type {
  ChatApi,
  Group,
  GroupMember,
  Message,
} from '../../src/lib/features/chat/models';
import type { Attachment, MediaApi } from '../../src/lib/features/media/models';
import { MediaUploads } from '../../src/lib/features/media/uploads.svelte';
import { SettingsSessions } from '../../src/lib/features/settings/sessions/sessions.svelte';

const cleanups: Array<() => void> = [];

afterEach(() => {
  for (const cleanup of cleanups.splice(0)) cleanup();
  vi.useRealTimers();
});

async function authenticatedFixture() {
  const queryClient = createQueryClient();
  const message: Message = {
    id: 'message',
    conversation_id: 'group',
    sender_id: 'alice',
    client_message_id: 'client-message',
    body: 'Saved history',
    created_seq: '1',
    revision_seq: '1',
    withdrawn: false,
    reactions: [],
    created_at: '2026-10-15T00:00:00Z',
  };
  const group: Group = {
    id: 'group',
    kind: 'group',
    title: 'Team',
    member_count: 1,
    membership: { membership_id: 'episode', joined_seq: '1' },
    my_role: 'owner',
    latest_event_seq: '1',
    last_message: message,
    unread_count: 0,
    my_read_seq: '0',
    preferences: { pinned: false, muted: false, archived: false },
  };
  const member: GroupMember = {
    user: { id: 'alice', username: 'alice', display_name: 'Alice' },
    membership_id: 'episode',
    joined_seq: '1',
    read_seq: '0',
    role: 'owner',
  };
  const attachment: Attachment = {
    id: 'attachment',
    purpose: 'message',
    state: 'ready',
    filename: 'note.txt',
    content_type: 'text/plain',
    size_bytes: 4,
    width: null,
    height: null,
    sha256: 'hash',
    error_code: null,
  };
  const session: components['schemas']['SessionView'] = {
    id: 'device',
    device_label: 'Browser',
    created_at: '2026-10-15T00:00:00Z',
    expires_at: '2026-11-15T00:00:00Z',
    is_current: true,
  };
  const unexpectedRequest = async (): Promise<never> => {
    throw new Error(
      'Unexpected API operation in an idle authenticated lifetime',
    );
  };
  const mediaApi: MediaApi = {
    create: async () => ({
      attachment: { ...attachment, state: 'uploading' },
      attempt_id: 'attempt',
      url: 'https://media.invalid/object',
      headers: {},
      expires_at: '2026-10-15T00:10:00Z',
    }),
    complete: async () => attachment,
    renew: unexpectedRequest,
    status: unexpectedRequest,
    cancel: unexpectedRequest,
    download: unexpectedRequest,
  };
  const uploads = new MediaUploads({
    queryClient,
    api: mediaApi,
    authorized: () => true,
    transfer: async () => undefined,
  });
  const chatApi: ChatApi = {
    snapshot: async () => ({
      items: [message],
      snapshot_seq: '1',
      next_before: null,
      membership: group.membership,
    }),
    members: async () => [member],
    list: unexpectedRequest,
    conversation: unexpectedRequest,
    events: unexpectedRequest,
    send: unexpectedRequest,
    edit: unexpectedRequest,
    withdraw: unexpectedRequest,
    setReaction: unexpectedRequest,
    removeReaction: unexpectedRequest,
    search: unexpectedRequest,
    context: unexpectedRequest,
    preferences: unexpectedRequest,
    read: unexpectedRequest,
    typing: unexpectedRequest,
    status: unexpectedRequest,
  };
  const store = new ConversationStore({
    queryClient,
    api: chatApi,
    userId: 'alice',
    active: () => true,
    epoch: () => 1,
    selectedId: () => 'group',
    changed: () => undefined,
    messagesMerged: () => undefined,
    peerSelected: () => undefined,
    membershipChanged: (id) => uploads.forgetConversation(id),
    failed: (reason) => {
      throw new Error(reason);
    },
  });
  const settings = new SettingsSessions({
    queryClient,
    userId: 'alice',
    api: {
      sessions: async () => [session],
      revokeSession: unexpectedRequest,
    },
  });
  cleanups.push(() => {
    settings.stop();
    uploads.stop();
    store.stop();
  });

  queryClient.setQueryData(keys.conversations, [group]);
  await store.select(group.id, async () => undefined);
  await store.groups.load(group.id);
  await uploads.add([new File(['note'], attachment.filename)], {
    purpose: 'message',
    conversationId: group.id,
    membershipId: group.membership.membership_id,
  });
  await settings.load();

  return {
    queryClient,
    store,
    uploads,
    settings,
    group,
    message,
    member,
    attachment,
    session,
  };
}

test('manual canonical records remain available beyond the former five-minute GC window', async () => {
  vi.useFakeTimers();
  const {
    queryClient,
    store,
    uploads,
    settings,
    group,
    message,
    member,
    attachment,
    session,
  } = await authenticatedFixture();

  // QueryCache subscriptions bridge runes; they are not QueryObservers that pin
  // these manually applied records. An idle open lifetime must still retain them.
  await vi.advanceTimersByTimeAsync(6 * 60_000);

  expect.soft(queryClient.getQueryData(keys.conversations)).toEqual([group]);
  expect.soft(store.messages(group.id)).toEqual([message]);
  expect.soft(store.groups.members(group.id)).toEqual([member]);
  expect.soft(store.status(group.id).applied).toBe('1');
  expect.soft(uploads.readyIds(group.id)).toEqual([attachment.id]);
  expect.soft(settings.sessions).toEqual([session]);
});

test('explicit membership and session teardown still remove retained canonical records', async () => {
  vi.useFakeTimers();
  const { queryClient, store, uploads, settings, group } =
    await authenticatedFixture();
  const draft = uploads.entries[0];

  store.revokeMembership(group.id, group.membership.membership_id);

  expect(store.messages(group.id)).toEqual([]);
  expect(store.groups.members(group.id)).toEqual([]);
  expect(queryClient.getQueryData(keys.conversations)).toEqual([]);
  expect(uploads.entries).toEqual([]);

  uploads.stop();
  settings.stop();
  store.stop();
  await vi.advanceTimersByTimeAsync(6 * 60_000);

  expect(uploads.metadata(draft)).toBeUndefined();
  expect(settings.sessions).toEqual([]);
  expect(queryClient.getQueryCache().getAll()).toHaveLength(0);
});
