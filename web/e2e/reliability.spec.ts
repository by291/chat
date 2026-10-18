import { test, expect, signup } from './fixtures';
import {
  api,
  startWorker,
  eventually,
  sdk,
  type IntegrationStack,
} from '../tests/integration/support';
import { readFile } from 'node:fs/promises';
import { randomUUID } from 'node:crypto';
import type { Browser, Page } from '@playwright/test';

test.setTimeout(60000);

async function pair(page: Page, browser: Browser, stack: IntegrationStack) {
  const context = await browser.newContext();
  const peer = await context.newPage();
  const a = await signup(page, stack),
    b = await signup(peer, stack);
  await startWorker(stack);
  await expect(page.locator('.chat-shell')).toHaveAttribute(
    'data-connection',
    'online',
  );
  await expect(peer.locator('.chat-shell')).toHaveAttribute(
    'data-connection',
    'online',
  );
  await page.getByRole('button', { name: '发起聊天', exact: true }).click();
  await page.getByLabel('对方的用户名', { exact: true }).fill(b.username);
  await page.getByRole('button', { name: '查找用户', exact: true }).click();
  await page.getByRole('button', { name: '开始聊天', exact: true }).click();
  await expect(page).toHaveURL(/\/app\/chats\//);
  await expect(
    peer.getByRole('navigation', { name: '会话列表' }).getByRole('link'),
  ).toHaveCount(1);
  await peer
    .getByRole('navigation', { name: '会话列表' })
    .getByRole('link')
    .click();
  const id = page.url().split('/').at(-1)!;
  return { context, peer, a, b, id };
}

async function cookie(page: Page) {
  const c = (await page.context().cookies()).find(
    (c) => c.name === 'chat_session',
  )!;
  return `${c.name}=${c.value}`;
}

async function send(page: Page, body: string) {
  await page.getByRole('textbox', { name: '输入消息' }).fill(body);
  await page.getByRole('textbox', { name: '输入消息' }).press('Enter');
}

test('lost HTTP response retries the same message ID and saves exactly once', async ({
  page,
  browser,
  stack,
}) => {
  const { peer, context } = await pair(page, browser, stack);
  try {
    await stack.stopWorker();
    let clientId = '',
      lost = false;
    await page.route('**/api/v1/conversations/*/messages', async (route) => {
      if (route.request().method() === 'POST' && !lost) {
        lost = true;
        clientId = route.request().postDataJSON().client_message_id;
        await route.fetch();
        await route.abort('failed');
      } else await route.continue();
    });
    await send(page, '响应丢失后显式重试');
    await page.getByRole('button', { name: '发送失败，重试' }).click();
    await startWorker(stack);
    await expect(
      peer.getByRole('log').getByText('响应丢失后显式重试', { exact: true }),
    ).toHaveCount(1);
    expect(
      (
        await stack.db.query(
          'SELECT count(*)::int AS n FROM messages WHERE client_message_id=$1',
          [clientId],
        )
      ).rows[0].n,
    ).toBe(1);
    await expect(
      page.getByRole('log').getByText('响应丢失后显式重试', { exact: true }),
    ).toHaveCount(1);
  } finally {
    await context.close();
  }
});

test('offline browser recovers every message through continuous REST replay', async ({
  page,
  browser,
  stack,
}) => {
  const { peer, context } = await pair(page, browser, stack);
  try {
    await page.context().setOffline(true);
    for (const body of ['断线期间的一', '断线期间的二', '断线期间的三'])
      await send(peer, body);
    await expect(
      peer.getByRole('log').getByText('断线期间的三', { exact: true }),
    ).toBeVisible();
    await page.context().setOffline(false);
    await page.bringToFront();
    for (const body of ['断线期间的一', '断线期间的二', '断线期间的三'])
      await expect(
        page.getByRole('log').getByText(body, { exact: true }),
      ).toHaveCount(1, { timeout: 20000 });
  } finally {
    await page.context().setOffline(false);
    await context.close();
  }
});

test('publish-before-ack worker crash gives duplicate notices but one displayed message', async ({
  page,
  browser,
  stack,
}) => {
  const { peer, context } = await pair(page, browser, stack);
  try {
    await eventually(
      async () =>
        (
          await stack.db.query(
            'SELECT count(*)::int AS n FROM outbox WHERE completed_at IS NULL',
          )
        ).rows[0].n === 0,
    );
    await stack.stopWorker();
    await send(page, 'worker 崩溃后仍只有一条');
    await startWorker(stack, true);
    let marker: { job_id: string } | undefined;
    await eventually(async () => {
      try {
        marker = JSON.parse(await readFile(stack.publishedMarker, 'utf8'));
        return true;
      } catch {
        return false;
      }
    });
    await expect(
      peer
        .getByRole('log')
        .getByText('worker 崩溃后仍只有一条', { exact: true }),
    ).toHaveCount(1);
    await stack.stopWorker();
    await stack.db.query(
      "UPDATE outbox SET lease_until=now()-interval '1 second' WHERE job_id=$1",
      [marker!.job_id],
    );
    await startWorker(stack);
    await eventually(
      async () =>
        (
          await stack.db.query(
            'SELECT count(*)::int AS n FROM outbox WHERE completed_at IS NULL',
          )
        ).rows[0].n === 0,
    );
    await expect(
      peer
        .getByRole('log')
        .getByText('worker 崩溃后仍只有一条', { exact: true }),
    ).toHaveCount(1);
    expect(
      (await stack.db.query('SELECT count(*)::int AS n FROM messages')).rows[0]
        .n,
    ).toBe(1);
  } finally {
    await context.close();
  }
});

test('a message arriving while the history snapshot is delayed is replayed', async ({
  page,
  browser,
  stack,
}) => {
  const { peer, context, id } = await pair(page, browser, stack);
  let release = () => {};
  try {
    let captured = () => {};
    const pending = new Promise<void>((resolve) => (captured = resolve));
    const gate = new Promise<void>((resolve) => (release = resolve));
    await page.route(
      `**/api/v1/conversations/${id}/messages?*`,
      async (route) => {
        const response = await route.fetch();
        captured();
        await gate;
        await route.fulfill({ response });
      },
    );
    const reload = page.reload();
    await pending;
    await send(peer, '旧快照加载时的新消息');
    await expect(
      peer.getByRole('log').getByText('旧快照加载时的新消息', { exact: true }),
    ).toBeVisible();
    release();
    await reload;
    await expect(
      page.getByRole('log').getByText('旧快照加载时的新消息', { exact: true }),
    ).toHaveCount(1);
  } finally {
    release();
    await context.close();
  }
});

test('read receipt converges between two devices of one account', async ({
  page,
  browser,
  stack,
}) => {
  const { peer, context, a, id } = await pair(page, browser, stack);
  const device = await browser.newContext();
  try {
    const second = await device.newPage();
    await second.goto('/login');
    await second.getByLabel('用户名', { exact: true }).fill(a.username);
    await second.getByLabel('密码', { exact: true }).fill(a.password);
    await second.getByRole('button', { name: '登录', exact: true }).click();
    await expect(second).toHaveURL(/\/app/);
    await second.goto(`/app/chats/${id}`);
    for (const target of [page, second])
      await target.evaluate(() => {
        Object.defineProperty(document, 'visibilityState', {
          configurable: true,
          get: () => 'hidden',
        });
        document.dispatchEvent(new Event('visibilitychange'));
      });
    await send(peer, '两个设备共享已读位置');
    await expect(
      page.getByRole('log').getByText('两个设备共享已读位置', { exact: true }),
    ).toBeVisible();
    await expect(
      second
        .getByRole('log')
        .getByText('两个设备共享已读位置', { exact: true }),
    ).toBeVisible();
    await second.evaluate(() => {
      Reflect.deleteProperty(document, 'visibilityState');
      document.dispatchEvent(new Event('visibilitychange'));
    });
    await second.bringToFront();
    const session = await cookie(page);
    await expect
      .poll(
        async () =>
          (await (await api(stack, session, `/conversations/${id}`)).json())
            .my_read_seq,
      )
      .toBe('2');
    await expect(
      page
        .getByRole('navigation', { name: '会话列表' })
        .locator('.unread-badge'),
    ).toHaveCount(0);
  } finally {
    await device.close();
    await context.close();
  }
});

test('revoked session loses REST, old sockets and reconnect permission', async ({
  page,
  browser,
  stack,
}) => {
  const { peer, context, b } = await pair(page, browser, stack);
  const session = await cookie(peer),
    connection = sdk(stack, session);
  try {
    await connection.connect();
    const user = await (await api(stack, session, '/auth/me')).json();
    await connection.subscribe(`personal:#${user.id}`);
    let code = 0;
    connection.client.on('disconnected', (event) => (code = event.code));
    const devices = await (await api(stack, session, '/sessions')).json();
    const current = devices.find((s: { is_current: boolean }) => s.is_current);
    expect(
      (await api(stack, session, `/sessions/${current.id}`, 'DELETE')).status,
    ).toBe(204);
    expect((await api(stack, session, '/auth/me')).status).toBe(401);
    await eventually(async () => code === 4501);
    await expect(peer).toHaveURL(/\/login/);
    const rejected = sdk(stack, session);
    try {
      await expect(rejected.connect()).rejects.toBeDefined();
    } finally {
      rejected.stop();
    }
    expect(b.username).toBeTruthy();
  } finally {
    connection.stop();
    await context.close();
  }
});

test('Valkey outage rejects critical writes while committed history stays readable', async ({
  page,
  browser,
  stack,
}) => {
  const { peer, context, id } = await pair(page, browser, stack);
  let stopped = false;
  try {
    await send(page, 'Valkey 停止前的历史');
    await expect(
      peer.getByRole('log').getByText('Valkey 停止前的历史', { exact: true }),
    ).toBeVisible();
    const session = await cookie(page);
    await stack.service('stop', 'valkey');
    stopped = true;
    const response = await api(
      stack,
      session,
      `/conversations/${id}/messages`,
      'POST',
      { client_message_id: randomUUID(), body: '应当拒绝的写入' },
    );
    expect(response.status).toBe(503);
    expect((await response.json()).code).toBe('dependency_unavailable');
    const history = await api(stack, session, `/conversations/${id}/messages`);
    expect(history.status).toBe(200);
    expect(
      (await history.json()).items.map((m: { body: string }) => m.body),
    ).toContain('Valkey 停止前的历史');
  } finally {
    if (stopped) await stack.service('start', 'valkey');
    await context.close();
  }
});

test('Centrifugo outage still commits messages and worker delivers after restart', async ({
  page,
  browser,
  stack,
}) => {
  const { peer, context, id } = await pair(page, browser, stack);
  let stopped = false;
  try {
    await stack.service('stop', 'centrifugo');
    stopped = true;
    const response = await api(
      stack,
      await cookie(page),
      `/conversations/${id}/messages`,
      'POST',
      { client_message_id: randomUUID(), body: '实时服务恢复后的消息' },
    );
    expect(response.status).toBe(201);
    expect(
      (await stack.db.query('SELECT count(*)::int AS n FROM messages')).rows[0]
        .n,
    ).toBe(1);
    await stack.service('start', 'centrifugo');
    stopped = false;
    await expect(
      peer.getByRole('log').getByText('实时服务恢复后的消息', { exact: true }),
    ).toHaveCount(1, { timeout: 30000 });
    await eventually(
      async () =>
        (
          await stack.db.query(
            'SELECT count(*)::int AS n FROM outbox WHERE completed_at IS NULL',
          )
        ).rows[0].n === 0,
      30000,
    );
  } finally {
    if (stopped) await stack.service('start', 'centrifugo');
    await context.close();
  }
});

test('session revalidation on focus preserves an unsent draft', async ({
  page,
  browser,
  stack,
}) => {
  const { context } = await pair(page, browser, stack);
  try {
    await page
      .getByRole('textbox', { name: '输入消息' })
      .fill('刷新身份也不应该丢草稿');
    const checked = page.waitForResponse((response) =>
      response.url().endsWith('/api/v1/auth/me'),
    );
    await page.evaluate(() => window.dispatchEvent(new Event('focus')));
    await checked;
    await expect(page.getByRole('textbox', { name: '输入消息' })).toHaveValue(
      '刷新身份也不应该丢草稿',
    );
  } finally {
    await context.close();
  }
});

test('foreground restoration catches missed messages without reconnect or a periodic poll', async ({
  page,
  browser,
  stack,
}) => {
  const { context, peer, id } = await pair(page, browser, stack);
  try {
    await page.addInitScript(() => {
      const schedule = window.setInterval.bind(window);
      window.setInterval = ((
        handler: TimerHandler,
        delay?: number,
        ...args: unknown[]
      ) =>
        delay === 60000
          ? 0
          : schedule(handler, delay, ...args)) as typeof window.setInterval;
    });
    await page.reload();
    await expect(page.getByRole('textbox', { name: '输入消息' })).toBeVisible();
    await expect(page.locator('.chat-shell')).toHaveAttribute(
      'data-connection',
      'online',
    );
    let reconnects = 0;
    page.on('websocket', () => reconnects++);
    await stack.stopWorker();
    const result = await api(
      stack,
      await cookie(peer),
      `/conversations/${id}/messages`,
      'POST',
      {
        client_message_id: randomUUID(),
        body: '前台恢复补齐遗漏',
      },
    );
    expect(result.status).toBe(201);
    await page.evaluate(() => window.dispatchEvent(new Event('focus')));
    await expect(
      page.getByRole('log').getByText('前台恢复补齐遗漏', { exact: true }),
    ).toBeVisible();
    expect(reconnects).toBe(0);
  } finally {
    await context.close();
  }
});

test('older history keeps its scroll anchor and long Unicode messages do not overflow', async ({
  page,
  browser,
  stack,
}) => {
  const { peer, context, id } = await pair(page, browser, stack);
  try {
    const session = await cookie(peer);
    for (let index = 1; index <= 55; index++) {
      const result = await api(
        stack,
        session,
        `/conversations/${id}/messages`,
        'POST',
        { client_message_id: randomUUID(), body: `第${index}条历史消息` },
      );
      expect(result.status).toBe(201);
    }
    await page.reload();
    await expect(
      page.getByRole('log').getByText('第55条历史消息', { exact: true }),
    ).toBeVisible();
    await page.locator('.message-scroll').evaluate((el) => (el.scrollTop = 0));
    const anchor = page
      .getByRole('log')
      .getByText('第6条历史消息', { exact: true });
    const before = (await anchor.boundingBox())!.y;
    await page.getByRole('button', { name: '加载更早的消息' }).click();
    await expect(
      page.getByRole('log').getByText('第1条历史消息', { exact: true }),
    ).toHaveCount(1);
    expect(Math.abs((await anchor.boundingBox())!.y - before)).toBeLessThan(3);
    const body = '你好🙂🌱'.repeat(600);
    await page.bringToFront();
    await send(page, body);
    await expect(
      peer.getByRole('log').getByText(body, { exact: true }),
    ).toHaveCount(1);
    expect(
      await peer
        .locator('.chat-panel')
        .evaluate((el) => el.scrollWidth <= el.clientWidth + 1),
    ).toBe(true);
  } finally {
    await context.close();
  }
});
