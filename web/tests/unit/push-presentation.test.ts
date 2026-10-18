import { describe, expect, it, vi } from 'vitest';

import { showPush, openNotification } from '../../src/service-worker/push';

const id = '019a1d05-32c4-7000-9000-111111111111';
const payload = {
  schema_version: 1,
  tag: `chat:${id}`,
  generation: id,
  path: `/app/chats/${id}`,
};

describe('background push presentation', () => {
  it('shows only generic text and replaces repeated attempts by their stable tag', async () => {
    const showNotification = vi.fn().mockResolvedValue(undefined);

    await showPush(
      { showNotification },
      {
        ...payload,
        generation: '00000000-0000-0000-0000-000000000000',
        body: 'Secret body',
        sender: 'Secret person',
      },
    );

    expect(showNotification).toHaveBeenCalledWith(
      'Chat',
      expect.objectContaining({
        body: '你有新消息',
        tag: payload.tag,
        data: { path: payload.path },
      }),
    );
    expect(JSON.stringify(showNotification.mock.calls)).not.toContain('Secret');
  });

  it('malformed and unsafe payloads retain a visible generic app-entry fallback', async () => {
    for (const input of [
      undefined,
      { ...payload, schema_version: 2 },
      { ...payload, generation: 'invalid-generation' },
      { ...payload, tag: 'chat:invalid-tag' },
      { ...payload, path: 'https://evil.test/app' },
      { ...payload, path: '//evil.test/app' },
      { ...payload, path: '/api/v1/messages' },
      { ...payload, path: `/app/chats/${id}?token=secret` },
    ]) {
      const showNotification = vi.fn().mockResolvedValue(undefined);

      await showPush({ showNotification }, input);

      expect(showNotification).toHaveBeenCalledWith(
        'Chat',
        expect.objectContaining({ body: '你有新消息', data: { path: '/app' } }),
      );
    }
  });

  it('focuses an existing same-origin app and announces safe navigation without fetching records', async () => {
    const tab = {
      url: 'https://chat.example.test/app',
      focus: vi.fn().mockResolvedValue(undefined),
      postMessage: vi.fn(),
    };
    const other = {
      url: 'https://evil.test/app',
      focus: vi.fn(),
      postMessage: vi.fn(),
    };
    const clients = {
      matchAll: vi.fn().mockResolvedValue([other, tab]),
      openWindow: vi.fn(),
    };

    await openNotification(
      clients,
      { path: payload.path },
      'https://chat.example.test',
    );

    expect(tab.postMessage).toHaveBeenCalledWith({
      type: 'open-notification',
      path: payload.path,
    });
    expect(tab.focus).toHaveBeenCalledOnce();
    expect(other.focus).not.toHaveBeenCalled();
    expect(clients.openWindow).not.toHaveBeenCalled();
  });

  it('opens only an allowed same-origin route when no owned window exists', async () => {
    const clients = {
      matchAll: vi.fn().mockResolvedValue([]),
      openWindow: vi.fn().mockResolvedValue(undefined),
    };

    await openNotification(
      clients,
      { path: 'https://evil.test' },
      'https://chat.example.test',
    );

    expect(clients.openWindow).toHaveBeenCalledWith(
      'https://chat.example.test/app',
    );
  });
});
