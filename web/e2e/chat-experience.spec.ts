import { startChatWith } from './fixtures';
import { befriend } from '../tests/integration/support';
import { sessionCookie } from './fixtures';
import { randomUUID } from 'node:crypto';

import type { Browser, Page } from '@playwright/test';

import { test, expect, signup } from './fixtures';
import {
  account,
  api,
  startWorker,
  type IntegrationStack,
} from '../tests/integration/support';
import type { Conversation, Message } from '../src/lib/features/chat/models';

test.setTimeout(90000);

async function cookie(page: Page) {
  const session = (await page.context().cookies()).find(
    (entry) => entry.name === 'chat_session',
  );
  if (!session) throw new Error('Expected authenticated browser');
  return `${session.name}=${session.value}`;
}

async function json<T>(response: Response): Promise<T> {
  if (!response.ok)
    throw new Error(
      `Experience request failed: ${response.status} ${await response.text()}`,
    );
  return response.json() as Promise<T>;
}

async function summary(stack: IntegrationStack, page: Page, id: string) {
  return json<Conversation>(
    await api(stack, await cookie(page), `/conversations/${id}`),
  );
}

async function pair(page: Page, browser: Browser, stack: IntegrationStack) {
  const context = await browser.newContext();
  const peer = await context.newPage();
  try {
    const author = await signup(page, stack);
    const partner = await signup(peer, stack);
    await befriend(stack, await sessionCookie(page), await sessionCookie(peer));
    await startWorker(stack);
    await expect(page.locator('.chat-shell')).toHaveAttribute(
      'data-connection',
      'online',
    );
    await expect(peer.locator('.chat-shell')).toHaveAttribute(
      'data-connection',
      'online',
    );
    await startChatWith(page, partner.username);
    await expect(page).toHaveURL(/\/app\/chats\//);
    const id = page.url().split('/').at(-1)!;
    const peerConversation = peer
      .getByRole('navigation', { name: '会话列表' })
      .getByRole('link')
      .filter({ hasText: author.username });
    await expect(peerConversation).toBeVisible();
    await peerConversation.click();
    await expect(peer).toHaveURL(/\/app\/chats\//);
    return { context, peer, author, id };
  } catch (reason) {
    await context.close();
    throw reason;
  }
}

async function send(page: Page, body: string) {
  const composer = page.getByRole('textbox', { name: '输入消息' });
  await composer.fill(body);
  await composer.press('Enter');
  await expect(
    page.getByRole('log').locator('.message-body').filter({ hasText: body }),
  ).toHaveCount(1);
}

async function closeDialog(page: Page) {
  await page
    .getByRole('dialog')
    .getByRole('button', { name: '关闭', exact: true })
    .click();
}

test('replies follow edits and withdrawals across live and offline browsers without resurrecting content', async ({
  page,
  browser,
  stack,
}) => {
  const { context, peer, id } = await pair(page, browser, stack);
  try {
    await send(page, '原始中文消息');
    const original = (await summary(stack, page, id)).last_message!;
    const originalRow = (target: Page) =>
      target.locator(`[data-message-id="${original.id}"]`);
    await expect(originalRow(peer).locator('.message-body')).toHaveText(
      '原始中文消息',
    );
    await expect(
      originalRow(peer).getByRole('button', { name: '编辑', exact: true }),
    ).toHaveCount(0);
    await originalRow(peer)
      .getByRole('button', { name: '回复', exact: true })
      .click();
    await send(peer, '对最早消息的回复');
    const reply = (await summary(stack, peer, id)).last_message!;
    const replyRow = (target: Page) =>
      target.locator(`[data-message-id="${reply.id}"]`);
    await expect(replyRow(page).locator('.reply-preview')).toContainText(
      '原始中文消息',
    );
    await page.screenshot({
      path: test.info().outputPath('experience-desktop-reply.png'),
    });

    await originalRow(page)
      .getByRole('button', { name: '编辑', exact: true })
      .click();
    await page.getByLabel('消息内容', { exact: true }).fill('第一次修改');
    await page.getByRole('button', { name: '保存修改', exact: true }).click();
    await expect(originalRow(peer).locator('.message-body')).toHaveText(
      '第一次修改',
    );
    await expect(replyRow(peer).locator('.reply-preview')).toContainText(
      '第一次修改',
    );
    for (const dismiss of await peer
      .getByRole('button', { name: '关闭提醒', exact: true })
      .all())
      await dismiss.click();

    await context.setOffline(true);
    await expect(peer.locator('.chat-shell')).not.toHaveAttribute(
      'data-connection',
      'online',
    );
    await originalRow(page)
      .getByRole('button', { name: '编辑', exact: true })
      .click();
    await page.getByLabel('消息内容', { exact: true }).fill('断线期间的修改');
    await page.getByRole('button', { name: '保存修改', exact: true }).click();
    await expect(originalRow(page).locator('.message-body')).toHaveText(
      '断线期间的修改',
    );
    await originalRow(page)
      .getByRole('button', { name: '撤回', exact: true })
      .click();
    await page.getByRole('button', { name: '确认撤回', exact: true }).click();
    await expect(originalRow(page).locator('.message-tombstone')).toHaveText(
      '此消息已撤回',
    );

    await context.setOffline(false);
    await expect(originalRow(peer).locator('.message-tombstone')).toHaveText(
      '此消息已撤回',
      { timeout: 20000 },
    );
    await expect(replyRow(peer).locator('.reply-preview')).toContainText(
      '此消息已撤回',
    );
    await expect(
      peer.getByRole('log').getByText('原始中文消息', { exact: true }),
    ).toHaveCount(0);
    await expect(
      peer.getByRole('log').getByText('断线期间的修改', { exact: true }),
    ).toHaveCount(0);
    await expect(peer.locator('.in-app-alert')).toHaveCount(0);
    await peer.reload();
    await expect(originalRow(peer).locator('.message-tombstone')).toBeVisible();
    await expect(replyRow(peer).locator('.reply-preview')).toContainText(
      '此消息已撤回',
    );
  } finally {
    await context.setOffline(false);
    await context.close();
  }
});

test('Chinese seek search and unloaded reply navigation preserve drafts and never read unseen latest history', async ({
  page,
  browser,
  stack,
}) => {
  const { context, peer, id } = await pair(page, browser, stack);
  try {
    await peer.addInitScript(() => {
      Object.defineProperty(document, 'visibilityState', {
        configurable: true,
        get: () => 'hidden',
      });
    });
    await peer.evaluate(() => {
      Object.defineProperty(document, 'visibilityState', {
        configurable: true,
        get: () => 'hidden',
      });
      document.dispatchEvent(new Event('visibilitychange'));
    });
    const authorCookie = await cookie(page);
    let first: Message | undefined;
    for (let index = 0; index < 65; index++) {
      const saved = await json<Message>(
        await api(
          stack,
          authorCookie,
          `/conversations/${id}/messages`,
          'POST',
          { client_message_id: randomUUID(), body: `中文历史 第${index}条` },
        ),
      );
      first ??= saved;
    }
    const reply = await json<Message>(
      await api(stack, authorCookie, `/conversations/${id}/messages`, 'POST', {
        client_message_id: randomUUID(),
        body: '引用最早的历史',
        reply_to_message_id: first!.id,
      }),
    );
    await peer.reload();
    const replyRow = peer.locator(`[data-message-id="${reply.id}"]`);
    await expect(replyRow).toBeVisible();
    await expect(peer.locator(`[data-message-id="${first!.id}"]`)).toHaveCount(
      0,
    );
    await peer
      .getByRole('textbox', { name: '输入消息' })
      .fill('跳转后仍保留的草稿');
    const reads: string[] = [];
    peer.on('request', (request) => {
      if (
        request.method() === 'POST' &&
        request.url().endsWith(`/conversations/${id}/read`)
      )
        reads.push(request.url());
    });

    await replyRow.getByRole('button', { name: '跳转到原消息' }).click();
    await expect(peer.locator('.context-target')).toContainText(
      '中文历史 第0条',
    );
    await expect(
      peer.getByRole('button', { name: '返回最新消息' }),
    ).toBeVisible();
    expect((await summary(stack, peer, id)).my_read_seq).toBe('0');
    await peer.getByRole('button', { name: '返回最新消息' }).click();
    await peer.getByRole('button', { name: '搜索聊天记录' }).click();
    await peer
      .getByRole('textbox', { name: '搜索消息', exact: true })
      .fill('中文历史');
    await peer.getByRole('button', { name: '搜索', exact: true }).click();
    await expect(peer.locator('.search-result')).toHaveCount(50);
    await peer
      .getByRole('button', { name: '加载更多结果', exact: true })
      .click();
    await expect(peer.locator('.search-result')).toHaveCount(65);
    await peer
      .locator('.search-result')
      .filter({ has: peer.getByText('中文历史 第0条', { exact: true }) })
      .click();
    await expect(peer.locator('.context-target')).toContainText(
      '中文历史 第0条',
    );
    await peer.evaluate(() => {
      Reflect.deleteProperty(document, 'visibilityState');
      document.hasFocus = () => true;
      document.dispatchEvent(new Event('visibilitychange'));
      window.dispatchEvent(new Event('focus'));
    });
    await peer.bringToFront();
    await peer.evaluate(
      () =>
        new Promise<void>((resolve) =>
          requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
        ),
    );
    expect(reads).toEqual([]);
    expect((await summary(stack, peer, id)).my_read_seq).toBe('0');
    await expect(peer.getByRole('textbox', { name: '输入消息' })).toHaveValue(
      '跳转后仍保留的草稿',
    );
    await peer.getByRole('button', { name: '返回最新消息' }).click();
    await expect
      .poll(
        async () =>
          BigInt((await summary(stack, peer, id)).my_read_seq) >=
          BigInt(reply.created_seq),
      )
      .toBe(true);
    await peer.setViewportSize({ width: 390, height: 844 });
    await peer.getByRole('button', { name: '搜索聊天记录' }).click();
    const bounds = await peer.getByRole('dialog').boundingBox();
    expect(bounds!.x).toBeGreaterThanOrEqual(0);
    expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(390);
    await peer.screenshot({
      path: test.info().outputPath('experience-mobile-search.png'),
    });
  } finally {
    await context.close();
  }
});

test('personal pin mute and archive synchronize through the worker to another account device and survive reload', async ({
  page,
  browser,
  stack,
}) => {
  const { context, peer, author, id } = await pair(page, browser, stack);
  const device = await browser.newContext();
  try {
    const other = await account(stack);
    await befriend(stack, await sessionCookie(page), other.cookie);
    await json<Conversation>(
      await api(stack, await cookie(page), '/conversations/direct', 'POST', {
        peer_id: other.user.id,
      }),
    );
    await expect(
      page.getByRole('navigation', { name: '会话列表' }).getByRole('link'),
    ).toHaveCount(2);
    const second = await device.newPage();
    await second.addInitScript(() => {
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
    await second.goto('/login');
    await second.getByLabel('用户名', { exact: true }).fill(author.username);
    await second.getByLabel('密码', { exact: true }).fill(author.password);
    await second.getByRole('button', { name: '登录', exact: true }).click();
    await expect(second).toHaveURL(/\/app/);
    await second.goto(`/app/chats/${id}`);
    await expect(second.locator('.chat-shell')).toHaveAttribute(
      'data-connection',
      'online',
    );
    await second.getByRole('button', { name: '会话设置', exact: true }).click();
    await page.getByRole('button', { name: '会话设置', exact: true }).click();
    for (const [before, after] of [
      ['置顶会话', '取消置顶'],
      ['静音会话', '取消静音'],
      ['归档会话', '移出归档'],
    ]) {
      await page.getByRole('button', { name: before, exact: true }).click();
      await expect(
        page.getByRole('button', { name: after, exact: true }),
      ).toBeVisible();
      await expect(
        second.getByRole('button', { name: after, exact: true }),
      ).toBeVisible();
      if (after === '取消置顶')
        await expect(
          page
            .getByRole('navigation', { name: '会话列表' })
            .getByRole('link')
            .first(),
        ).toHaveAttribute('href', `/app/chats/${id}`);
    }
    await closeDialog(page);
    await closeDialog(second);
    await expect(
      second
        .getByRole('navigation', { name: '会话列表' })
        .locator(`a[href="/app/chats/${id}"]`),
    ).toHaveCount(0);
    await second.getByRole('button', { name: '归档 (1)', exact: true }).click();
    await expect(
      second.getByRole('navigation', { name: '会话列表' }).getByRole('link'),
    ).toHaveCount(1);
    expect((await summary(stack, peer, id)).preferences).toEqual({
      pinned: false,
      muted: false,
      archived: false,
    });
    await page.getByRole('button', { name: '归档 (1)', exact: true }).click();
    await expect(
      page
        .getByRole('navigation', { name: '会话列表' })
        .getByRole('link')
        .first(),
    ).toHaveAttribute('href', `/app/chats/${id}`);
    await send(peer, '归档和静音保持不变');
    await expect(second.getByRole('log').locator('.message-body')).toHaveText(
      '归档和静音保持不变',
    );
    await expect(second.locator('.in-app-alert')).toHaveCount(0);
    await page.reload();
    await page.getByRole('button', { name: '归档 (1)', exact: true }).click();
    await expect(
      page.getByRole('navigation', { name: '会话列表' }).getByRole('link'),
    ).toHaveCount(1);
    expect((await summary(stack, page, id)).preferences).toEqual({
      pinned: true,
      muted: true,
      archived: true,
    });
  } finally {
    await device.close();
    await context.close();
  }
});

test('open-page alerts request permission only on click omit private bodies open their chat and clean up on logout', async ({
  page,
  browser,
  stack,
}) => {
  const { context, peer, id } = await pair(page, browser, stack);
  try {
    await page.addInitScript(() => {
      const state = {
        permissionRequests: 0,
        shown: [] as { title: string; body: string }[],
        soundStarts: 0,
        audioCloses: 0,
        lastClick: () => {},
      };
      Object.assign(window, { experienceNotifications: state });
      class BrowserNotification {
        static permission = 'default';
        static async requestPermission() {
          state.permissionRequests++;
          return 'granted';
        }
        onclick = () => {};
        onclose = () => {};
        constructor(title: string, options: { body: string }) {
          state.shown.push({ title, body: options.body });
          state.lastClick = () => this.onclick();
        }
        close() {
          this.onclose();
        }
      }
      class BrowserAudio {
        state = 'running';
        currentTime = 0;
        destination = {};
        async resume() {}
        async close() {
          state.audioCloses++;
        }
        createOscillator() {
          return {
            frequency: { value: 0 },
            connect() {},
            disconnect() {},
            start() {
              state.soundStarts++;
            },
            stop() {},
            onended: () => {},
          };
        }
        createGain() {
          return {
            gain: { setValueAtTime() {}, exponentialRampToValueAtTime() {} },
            connect() {},
            disconnect() {},
          };
        }
      }
      Object.assign(window, {
        Notification: BrowserNotification,
        AudioContext: BrowserAudio,
      });
    });
    await page.goto('/app');
    await expect(page.locator('.chat-shell')).toHaveAttribute(
      'data-connection',
      'online',
    );
    const state = () =>
      page.evaluate(
        () =>
          (
            window as unknown as {
              experienceNotifications: {
                permissionRequests: number;
                shown: { title: string; body: string }[];
                soundStarts: number;
                audioCloses: number;
              };
            }
          ).experienceNotifications,
      );
    expect((await state()).permissionRequests).toBe(0);
    expect((await state()).soundStarts).toBe(0);
    await page.getByRole('button', { name: '通知设置', exact: true }).click();
    await page
      .getByRole('button', { name: '开启系统通知', exact: true })
      .click();
    await page.getByRole('button', { name: '开启提示音', exact: true }).click();
    await closeDialog(page);
    await page.getByRole('link', { name: '设置', exact: true }).click();
    await expect(
      page.getByRole('heading', { name: '设置', exact: true }),
    ).toBeVisible();

    const incoming = await json<Message>(
      await api(
        stack,
        await cookie(peer),
        `/conversations/${id}/messages`,
        'POST',
        { client_message_id: randomUUID(), body: '系统通知不可泄露的正文' },
      ),
    );
    await expect(page.locator('.in-app-alert')).toHaveCount(1);
    await expect.poll(async () => (await state()).shown.length).toBe(1);
    expect((await state()).shown[0].body).toBe('打开聊天查看');
    expect(JSON.stringify((await state()).shown)).not.toContain(incoming.body);
    expect((await state()).permissionRequests).toBe(1);
    expect((await state()).soundStarts).toBe(1);
    await page.evaluate(() =>
      (
        window as unknown as {
          experienceNotifications: { lastClick: () => void };
        }
      ).experienceNotifications.lastClick(),
    );
    await expect(page).toHaveURL(new RegExp(`/app/chats/${id}$`));
    await expect(
      page.locator(`[data-message-id="${incoming.id}"]`),
    ).toBeVisible();
    await page.evaluate(() => {
      document.hasFocus = () => true;
      window.dispatchEvent(new Event('focus'));
    });
    const focused = await json<Message>(
      await api(
        stack,
        await cookie(peer),
        `/conversations/${id}/messages`,
        'POST',
        { client_message_id: randomUUID(), body: '正在查看时保持安静' },
      ),
    );
    await expect(
      page.locator(`[data-message-id="${focused.id}"]`),
    ).toBeVisible();
    expect((await state()).shown).toHaveLength(1);
    await page.getByRole('link', { name: '设置', exact: true }).click();
    await expect(
      page.getByRole('heading', { name: '设置', exact: true }),
    ).toBeVisible();
    const fromSettings = await json<Message>(
      await api(
        stack,
        await cookie(peer),
        `/conversations/${id}/messages`,
        'POST',
        { client_message_id: randomUUID(), body: '设置页面也能打开提醒' },
      ),
    );
    await expect(page.locator('.in-app-alert')).toHaveCount(1);
    await page.locator('.alert-open').click();
    await expect(page).toHaveURL(new RegExp(`/app/chats/${id}$`));
    await expect(
      page.locator(`[data-message-id="${fromSettings.id}"]`),
    ).toBeVisible();
    await page.getByRole('button', { name: '我的账号', exact: true }).click();
    await page.getByRole('menuitem', { name: '退出登录', exact: true }).click();
    await expect(page).toHaveURL(/\/login/);
    expect((await state()).audioCloses).toBe(1);
    await expect(page.locator('.in-app-alert')).toHaveCount(0);
  } finally {
    await context.close();
  }
});
