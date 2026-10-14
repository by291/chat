import { test as base, expect, type BrowserContext } from '@playwright/test';

import { TestPushProvider } from '../tests/integration/push-support';
import { TestRuntime } from '../tests/integration/runtime';
import { account, api } from '../tests/integration/support';
import { signup } from './fixtures';

const test = base.extend<{ provider: TestPushProvider; stack: TestRuntime }>({
  provider: async ({}, use) => {
    const provider = new TestPushProvider();
    await provider.start();
    try {
      await use(provider);
    } finally {
      await provider.stop();
    }
  },
  stack: [
    async ({ provider }, use) => {
      const stack = await TestRuntime.start({ push: provider.environment() });
      try {
        await stack.startWeb({ mode: 'preview' });
        await use(stack);
      } finally {
        await stack.cleanup();
      }
    },
    { timeout: 120000 },
  ],
});

// Full Chromium's new headless mode supports browser notifications; the default
// headless shell omits that capability even when test permissions are granted.
const notificationChannel =
  process.env.CHAT_TEST_PUSH_BROWSER_CHANNEL ?? 'chromium';
if (!['chromium', 'chrome', 'msedge'].includes(notificationChannel))
  throw new Error('Invalid push test browser channel');
test.use({ channel: notificationChannel });

test('push controls recover after a delayed first service-worker registration without a reload', async ({
  page,
  context,
  stack,
  provider,
}) => {
  await controlledCapability(context, provider);
  const user = await account(stack);
  const [name, value] = user.cookie.split('=');
  await context.addCookies([{ name, value, url: stack.origin }]);
  let release!: () => void;
  let requested!: () => void;
  const held = new Promise<void>((resolve) => (release = resolve));
  const requesting = new Promise<void>((resolve) => (requested = resolve));
  // SvelteKit registers at window.load. A genuine pending image keeps that event
  // behind auth/config, without replacing getRegistration or the worker itself.
  await context.addInitScript(() => {
    document.addEventListener(
      'DOMContentLoaded',
      () => {
        const image = document.createElement('img');
        image.hidden = true;
        image.src = '/icons/icon-192.png?initial-worker-load';
        document.body.append(image);
      },
      { once: true },
    );
  });
  await context.route(
    '**/icons/icon-192.png?initial-worker-load',
    async (route) => {
      requested();
      await held;
      await route.continue();
    },
  );

  try {
    const configured = page.waitForResponse('**/api/v1/notifications/config');
    await page.goto('/app/settings', { waitUntil: 'domcontentloaded' });
    await requesting;
    await configured;
    await expect(
      page.getByRole('region', { name: '后台推送' }).getByRole('button'),
    ).toBeDisabled();
    release();

    await expect
      .poll(() =>
        page.evaluate(
          async () =>
            (await navigator.serviceWorker.getRegistration())?.active?.state,
        ),
      )
      .toBe('activated');
    await expect(
      page.getByRole('button', { name: '启用后台推送' }),
    ).toBeEnabled();
  } finally {
    release();
  }
});

test('an enabled device survives a same-session SPA trip out of the application layout', async ({
  page,
  context,
  stack,
  provider,
}) => {
  await controlledCapability(context, provider);
  await signup(page, stack);
  await expect
    .poll(() =>
      page.evaluate(
        async () =>
          (await navigator.serviceWorker.getRegistration())?.active?.state,
      ),
    )
    .toBe('activated');
  await page.goto('/login');
  await expect(page).toHaveURL(/\/app/);
  await page.getByRole('link', { name: '设置', exact: true }).click();
  await page.getByRole('button', { name: '启用后台推送' }).click();
  await expect(
    page.getByRole('button', { name: '关闭后台推送' }),
  ).toBeEnabled();

  // A normal link uses SvelteKit navigation and disposes /app without ending auth.
  await page.evaluate(() => {
    const link = document.createElement('a');
    link.href = '/login';
    document.body.append(link);
    link.click();
    link.remove();
  });
  await expect(page).toHaveURL(/\/app$/);
  await page.getByRole('link', { name: '设置', exact: true }).click();

  await expect(
    page.getByRole('button', { name: '关闭后台推送' }),
  ).toBeEnabled();
  expect(
    await page.evaluate(
      async () =>
        !!(await (await navigator.serviceWorker.getRegistration())!.pushManager.getSubscription()),
    ),
  ).toBe(true);
  expect(
    (
      await stack.db.query(
        'SELECT count(*)::int AS count FROM push_subscriptions WHERE revoked_at IS NULL',
      )
    ).rows[0].count,
  ).toBe(1);
});

async function controlledCapability(
  context: BrowserContext,
  provider: TestPushProvider,
) {
  const input = provider.subscription('browser');
  let subscribed = false;
  await context.exposeBinding(
    '__ownedPushCapability',
    (_, operation: string) => {
      if (operation === 'subscribe') subscribed = true;
      if (operation === 'unsubscribe') subscribed = false;
      return subscribed ? input : null;
    },
  );
  await context.grantPermissions(['notifications']);
  await context.addInitScript(() => {
    const fixture = window as unknown as {
      __ownedPushCapability(
        operation: string,
      ): Promise<{ endpoint: string; p256dh: string; auth: string } | null>;
    };
    const subscription = (input: {
      endpoint: string;
      p256dh: string;
      auth: string;
    }) => ({
      endpoint: input.endpoint,
      expirationTime: null,
      options: { userVisibleOnly: true, applicationServerKey: null },
      getKey: (name: PushEncryptionKeyName) => {
        const encoded = name === 'p256dh' ? input.p256dh : input.auth;
        const decoded = atob(encoded.replace(/-/g, '+').replace(/_/g, '/'));
        const bytes = new Uint8Array(decoded.length);
        for (let index = 0; index < decoded.length; index++)
          bytes[index] = decoded.charCodeAt(index);
        return bytes.buffer;
      },
      toJSON: () => ({
        endpoint: input.endpoint,
        keys: { p256dh: input.p256dh, auth: input.auth },
      }),
      unsubscribe: async () => {
        await fixture.__ownedPushCapability('unsubscribe');
        return true;
      },
    });
    PushManager.prototype.getSubscription = async () => {
      const input = await fixture.__ownedPushCapability('get');
      return input ? (subscription(input) as PushSubscription) : null;
    };
    PushManager.prototype.subscribe = async () =>
      subscription(
        (await fixture.__ownedPushCapability('subscribe'))!,
      ) as PushSubscription;
  });
}

test('controlled browser enrollment uses real REST, encrypted TLS delivery and actual worker presentation', async ({
  page,
  context,
  stack,
  provider,
}) => {
  test.setTimeout(180000);
  await controlledCapability(context, provider);
  await context.grantPermissions(['notifications'], { origin: stack.origin });
  await signup(page, stack);
  await expect
    .poll(() =>
      page.evaluate(
        async () =>
          (await navigator.serviceWorker.getRegistration())?.active?.state,
      ),
    )
    .toBe('activated');
  await page.goto('/app/settings');
  expect(await page.evaluate(() => Notification.permission)).toBe('granted');
  await expect(
    page.getByRole('button', { name: '启用后台推送' }),
  ).toBeEnabled();
  expect(
    (
      await stack.db.query(
        'SELECT count(*)::int AS count FROM push_subscriptions',
      )
    ).rows[0].count,
  ).toBe(0);
  await page.getByRole('button', { name: '启用后台推送' }).click();
  await expect(
    page.getByRole('button', { name: '关闭后台推送' }),
  ).toBeEnabled();
  await stack.startWorker();
  const peer = await account(stack);
  const receiverCookie = (await context.cookies()).find(
    (cookie) => cookie.name === 'chat_session',
  )!;
  const receiver = `${receiverCookie.name}=${receiverCookie.value}`;
  const current = await (await api(stack, receiver, '/auth/me')).json();
  const conversation = await (
    await api(stack, peer.cookie, '/conversations/direct', 'POST', {
      peer_id: current.id,
    })
  ).json();
  const sent = await (
    await api(
      stack,
      peer.cookie,
      `/conversations/${conversation.id}/messages`,
      'POST',
      {
        client_message_id: crypto.randomUUID(),
        body: 'Private body must remain absent from push',
      },
    )
  ).json();

  await expect
    .poll(() => provider.requests().length, { timeout: 20000 })
    .toBe(1);
  const captured = provider.requests()[0];
  expect(captured.vapidValid).toBe(true);
  expect(JSON.stringify(captured.payload)).not.toContain(sent.body);
  const payload = captured.payload as { path: string };
  expect(payload.path).toBe(`/app/chats/${conversation.id}`);
  const cdp = await context.newCDPSession(page);
  let registrationId: string | undefined;
  cdp.on('ServiceWorker.workerRegistrationUpdated', ({ registrations }) => {
    registrationId =
      registrations.find(
        (registration) =>
          registration.scopeURL === stack.origin + '/' &&
          !registration.isDeleted,
      )?.registrationId ?? registrationId;
  });
  await cdp.send('ServiceWorker.enable');
  await expect.poll(() => registrationId).toBeTruthy();
  for (let attempt = 0; attempt < 2; attempt++) {
    await cdp.send('ServiceWorker.deliverPushMessage', {
      origin: stack.origin,
      registrationId: registrationId!,
      data: JSON.stringify(captured.payload),
    });
  }
  await expect
    .poll(() =>
      page.evaluate(async () => {
        const notifications =
          await (await navigator.serviceWorker.getRegistration())!.getNotifications();
        return notifications.map((notification) => ({
          title: notification.title,
          body: notification.body,
          data: notification.data,
        }));
      }),
    )
    .toEqual([
      { title: 'Chat', body: '你有新消息', data: { path: payload.path } },
    ]);
  const unread = await stack.db.query(
    'SELECT last_read_message_seq FROM conversation_members WHERE user_id=$1',
    [current.id],
  );
  expect(unread.rows[0].last_read_message_seq).toBe('0');
  const worker = context.serviceWorkers()[0];
  await worker.evaluate(async (path) => {
    const worker = self as unknown as {
      clients: {
        matchAll(options: {
          type: 'window';
        }): Promise<Array<{ postMessage(message: unknown): void }>>;
      };
    };
    const clients = await worker.clients.matchAll({ type: 'window' });
    clients[0].postMessage({ type: 'open-notification', path });
  }, payload.path);
  await expect(page).toHaveURL(new RegExp(`/app/chats/${conversation.id}$`));
  await expect(
    page
      .getByRole('log', { name: '聊天消息' })
      .getByText(sent.body, { exact: true }),
  ).toHaveCount(1);
  const privateCaches = await page.evaluate(async () => {
    const urls: string[] = [];
    for (const name of await caches.keys())
      for (const request of await (await caches.open(name)).keys())
        urls.push(request.url);
    return urls.filter(
      (url) => url.includes('/api/') || url.includes('/app/chats/'),
    );
  });
  expect(privateCaches).toEqual([]);

  await page.getByRole('button', { name: '退出登录' }).click();
  await expect(page).toHaveURL('/login');
  await expect
    .poll(() =>
      page.evaluate(
        async () =>
          (
            await (await navigator.serviceWorker.getRegistration())!.getNotifications()
          ).length,
      ),
    )
    .toBe(0);
  const old = await stack.db.query(
    'SELECT generation,revoked_at FROM push_subscriptions',
  );
  expect(old.rows[0].revoked_at).not.toBeNull();
  await signup(page, stack);
  await page.goto('/app/settings');
  await page.getByRole('button', { name: '启用后台推送' }).click();
  await expect(
    page.getByRole('button', { name: '关闭后台推送' }),
  ).toBeEnabled();
  const rebound = await stack.db.query(
    'SELECT generation FROM push_subscriptions WHERE revoked_at IS NULL',
  );
  expect(rebound.rows).toHaveLength(1);
  expect(rebound.rows[0].generation).not.toBe(old.rows[0].generation);
});
