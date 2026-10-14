import { afterEach, describe, expect, it, vi } from 'vitest';

import { PushSubscriptions } from '../../src/lib/features/notifications/subscriptions.svelte';
import type { NotificationApi } from '../../src/lib/features/notifications/api';

const publicKey = 'B' + 'A'.repeat(86);
const binding = {
  id: '019a1d05-32c4-7000-9000-111111111111',
  generation: '019a1d05-32c4-7000-9000-222222222222',
};

function fixture(
  initial: NotificationPermission = 'default',
  existing = false,
) {
  let permission = initial;
  let current = existing;
  let focused = true;
  const subscription = {
    toJSON: () => ({
      endpoint: 'https://push.example.test/send',
      keys: { p256dh: publicKey, auth: 'A'.repeat(22) },
    }),
    unsubscribe: vi.fn(async () => {
      current = false;
      return true;
    }),
  };
  const notifications = [{ close: vi.fn() }];
  const registration = {
    pushManager: {
      getSubscription: vi.fn(async () => (current ? subscription : null)),
      subscribe: vi.fn(async () => {
        current = true;
        return subscription;
      }),
    },
    getNotifications: vi.fn(async () => notifications),
  };
  const events = new EventTarget();
  const documentEvents = new EventTarget();
  const requestPermission = vi.fn<() => Promise<NotificationPermission>>(
    async () => (permission = 'granted'),
  );
  const ports = {
    events,
    documentEvents,
    permission: () => permission,
    requestPermission,
    registration: async () => registration,
    activity: () => ({ visible: true, focused }),
  };
  const api = {
    config: vi.fn(async () => ({ enabled: true, vapid_public_key: publicKey })),
    register: vi.fn<NotificationApi['register']>(async () => binding),
    remove: vi.fn<NotificationApi['remove']>(async () => {}),
    activity: vi.fn<NotificationApi['activity']>(async () => {}),
  };
  const owner = new PushSubscriptions(api, ports);
  return {
    owner,
    api,
    ports,
    registration,
    subscription,
    notifications,
    setFocus: (value: boolean) => {
      focused = value;
    },
  };
}

afterEach(() => vi.useRealTimers());

describe('session-owned background subscriptions', () => {
  it('ordinary disposal preserves a consented capability for the successor owner', async () => {
    const { owner, api, ports, subscription } = fixture('granted', true);
    await owner.start();

    owner.dispose();
    const successor = new PushSubscriptions(api, ports);
    await successor.start();

    expect(subscription.unsubscribe).not.toHaveBeenCalled();
    expect(successor.enabled).toBe(true);
    await successor.stop();
  });

  it('a disposed readiness wait cannot enroll a later worker in the old lifetime', async () => {
    const { owner, api, ports, registration } = fixture('granted', true);
    let resolve!: (value: typeof registration) => void;
    ports.registration = () => new Promise((ready) => (resolve = ready));
    const starting = owner.start();
    await vi.waitFor(() => expect(resolve).toBeDefined());

    owner.dispose();
    resolve(registration);
    await starting;

    expect(api.register).not.toHaveBeenCalled();
    expect(owner.enabled).toBe(false);
  });

  it('logout still clears an existing browser capability when the server has disabled push', async () => {
    const { owner, api, subscription, notifications } = fixture(
      'granted',
      true,
    );
    api.config.mockResolvedValue({
      enabled: false,
      vapid_public_key: publicKey,
    });
    await owner.start();

    await owner.stop();

    expect(api.register).not.toHaveBeenCalled();
    expect(subscription.unsubscribe).toHaveBeenCalled();
    expect(notifications[0].close).toHaveBeenCalled();
  });

  it('a notification lookup failure does not prevent browser unsubscribe after server removal', async () => {
    const { owner, api, registration, subscription } = fixture('granted', true);
    await owner.start();
    registration.getNotifications.mockRejectedValue(
      new Error('Notification lookup failed'),
    );

    await owner.disable();

    expect(api.remove).toHaveBeenCalled();
    expect(subscription.unsubscribe).toHaveBeenCalled();
    expect(owner.enabled).toBe(false);
    expect(owner.error).not.toBe('');
    await owner.stop();
  });

  it('starts without a permission request or silent enrollment', async () => {
    const { owner, ports, registration, api } = fixture();

    await owner.start();

    expect(ports.requestPermission).not.toHaveBeenCalled();
    expect(registration.pushManager.subscribe).not.toHaveBeenCalled();
    expect(api.register).not.toHaveBeenCalled();
    await owner.stop();
  });

  it('requests permission synchronously from enable and reports denial without enrollment', async () => {
    const { owner, ports, registration } = fixture();
    await owner.start();
    ports.requestPermission.mockResolvedValue('denied');

    const enabling = owner.enable();
    expect(ports.requestPermission).toHaveBeenCalledOnce();
    await enabling;

    expect(owner.enabled).toBe(false);
    expect(owner.permission).toBe('denied');
    expect(registration.pushManager.subscribe).not.toHaveBeenCalled();
    await owner.stop();
  });

  it('rebinds an existing granted browser capability to the current authenticated session', async () => {
    const { owner, api, ports } = fixture('granted', true);

    await owner.start();

    expect(owner.enabled).toBe(true);
    expect(api.register).toHaveBeenCalledWith(
      expect.objectContaining({
        endpoint: 'https://push.example.test/send',
        p256dh: publicKey,
      }),
      expect.any(AbortSignal),
    );
    expect(ports.requestPermission).not.toHaveBeenCalled();
    await owner.stop();
  });

  it('fences a late registration acknowledgement and unsubscribes its disposed capability', async () => {
    const { owner, api, subscription } = fixture('granted');
    let resolveRegistration!: (value: typeof binding) => void;
    api.register.mockImplementation(
      () => new Promise((resolve) => (resolveRegistration = resolve)),
    );
    await owner.start();
    const enabling = owner.enable();
    await vi.waitFor(() => expect(api.register).toHaveBeenCalledOnce());
    const stopping = owner.stop();

    resolveRegistration(binding);
    await enabling;
    await stopping;

    expect(owner.enabled).toBe(false);
    expect(subscription.unsubscribe).toHaveBeenCalled();
  });

  it('renews every ten seconds, releases on blur and clears listeners and timers on stop', async () => {
    vi.useFakeTimers();
    const { owner, api, ports, setFocus, notifications } = fixture(
      'granted',
      true,
    );
    await owner.start();
    owner.setActivity({
      visible: true,
      focused: true,
      conversation_id: binding.id,
    });
    await vi.advanceTimersByTimeAsync(10000);
    expect(api.activity.mock.calls.at(-1)?.[1]).toEqual({
      visible: true,
      focused: true,
      conversation_id: binding.id,
    });

    setFocus(false);
    ports.events.dispatchEvent(new Event('blur'));
    await vi.advanceTimersByTimeAsync(0);
    expect(api.activity.mock.calls.at(-1)?.[1]?.focused).toBe(false);
    await owner.stop();
    const count = api.activity.mock.calls.length;
    await vi.advanceTimersByTimeAsync(30000);
    ports.events.dispatchEvent(new Event('focus'));

    expect(api.activity).toHaveBeenCalledTimes(count);
    expect(notifications[0].close).toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);
  });
});
