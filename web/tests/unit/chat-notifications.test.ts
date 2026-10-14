import { afterEach, expect, test, vi } from 'vitest';

import {
  ChatNotifications,
  shouldNotify,
} from '../../src/lib/features/chat/notifications.svelte';
import type { Conversation, Message } from '../../src/lib/features/chat/models';

afterEach(() => {
  vi.unstubAllGlobals();
});

const incoming: Message = {
  id: 'message',
  conversation_id: 'chat',
  sender_id: 'bob',
  client_message_id: 'client',
  body: 'private body never shown in a system alert',
  created_at: '2026-10-14T00:00:00Z',
  created_seq: '2',
  revision_seq: '2',
  withdrawn: false,
};
const conversation: Conversation = {
  id: 'chat',
  peer: { id: 'bob', username: 'bob', display_name: '伙伴' },
  latest_event_seq: '2',
  my_read_seq: '0',
  peer_read_seq: '0',
  unread_count: 1,
  preferences: { pinned: false, muted: false, archived: false },
};

test.each([
  [
    {
      fresh: true,
      incoming: true,
      withdrawn: false,
      muted: false,
      focusedVisibleCurrent: false,
    },
    true,
  ],
  [
    {
      fresh: false,
      incoming: true,
      withdrawn: false,
      muted: false,
      focusedVisibleCurrent: false,
    },
    false,
  ],
  [
    {
      fresh: true,
      incoming: false,
      withdrawn: false,
      muted: false,
      focusedVisibleCurrent: false,
    },
    false,
  ],
  [
    {
      fresh: true,
      incoming: true,
      withdrawn: true,
      muted: false,
      focusedVisibleCurrent: false,
    },
    false,
  ],
  [
    {
      fresh: true,
      incoming: true,
      withdrawn: false,
      muted: true,
      focusedVisibleCurrent: false,
    },
    false,
  ],
  [
    {
      fresh: true,
      incoming: true,
      withdrawn: false,
      muted: false,
      focusedVisibleCurrent: true,
    },
    false,
  ],
])('notification policy %j yields %s', (policy, expected) => {
  expect(shouldNotify(policy)).toBe(expected);
});

test.each(['denied', 'unsupported'])(
  '%s system permissions retain in-app alerts without requesting permission on mount',
  async (capability) => {
    const notifications = new ChatNotifications({
      userId: 'alice',
      selectedId: () => null,
      visible: () => false,
    });
    const permissionRequest = vi.fn(async () => 'denied');
    vi.stubGlobal(
      'Notification',
      capability === 'unsupported'
        ? undefined
        : {
            permission: 'denied',
            requestPermission: permissionRequest,
          },
    );
    notifications.mount();
    expect(permissionRequest).not.toHaveBeenCalled();

    await notifications.toggleSystem();
    notifications.applied(incoming, conversation, true);

    expect(notifications.system).toBe(false);
    expect(notifications.items.map((item) => item.messageId)).toEqual([
      'message',
    ]);
    expect(notifications.feedback).toContain('页面内提醒');
    notifications.stop();
  },
);

test('system alert omits message content, opens the conversation and loses its callback after logout', async () => {
  const shown: FakeNotification[] = [];
  class FakeNotification {
    static permission = 'default';
    static async requestPermission() {
      return 'granted';
    }
    onclick?: () => void;
    onclose?: () => void;
    constructor(
      readonly title: string,
      readonly options: { body: string; tag: string },
    ) {
      shown.push(this);
    }
    close() {
      this.onclose?.();
    }
  }
  vi.stubGlobal('Notification', FakeNotification);
  vi.stubGlobal('window', { focus: () => {} });
  const opened: string[] = [];
  const notifications = new ChatNotifications({
    userId: 'alice',
    selectedId: () => null,
    visible: () => false,
  });
  notifications.mount((id) => opened.push(id));
  await notifications.toggleSystem();

  notifications.applied(incoming, conversation, true);
  notifications.applied(incoming, conversation, true);

  expect(shown).toHaveLength(1);
  expect(shown[0].options.body).toBe('打开聊天查看');
  expect(shown[0].title).not.toContain(incoming.body);
  shown[0].onclick?.();
  expect(opened).toEqual(['chat']);
  notifications.stop();
  shown[0].onclick?.();
  notifications.applied({ ...incoming, id: 'another' }, conversation, true);
  expect(opened).toEqual(['chat']);
  expect(notifications.items).toEqual([]);
});

test('notification state and retained system alerts are bounded and cleared on stop', () => {
  const notifications = new ChatNotifications({
    userId: 'alice',
    selectedId: () => null,
    visible: () => false,
  });
  for (let index = 0; index < 220; index++)
    notifications.applied(
      { ...incoming, id: `message-${index}` },
      conversation,
      true,
    );

  expect(notifications.items).toHaveLength(5);
  expect(notifications.items[0].messageId).toBe('message-215');
  notifications.stop();
  expect(notifications.items).toEqual([]);
});

test.each(['muted', 'focused'])(
  'a duplicate hint cannot revive an alert suppressed while %s',
  (suppression) => {
    let focused = suppression === 'focused';
    const notifications = new ChatNotifications({
      userId: 'alice',
      selectedId: () => 'chat',
      visible: () => focused,
    });
    const suppressed = {
      ...conversation,
      preferences: {
        ...conversation.preferences,
        muted: suppression === 'muted',
      },
    };
    notifications.applied(incoming, suppressed, true);

    focused = false;
    notifications.applied(incoming, conversation, true);

    expect(notifications.items).toEqual([]);
    notifications.stop();
  },
);
