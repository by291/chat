import { test, expect, signup } from './fixtures';
import { api, startWorker } from '../tests/integration/support';
import type { Page, Browser } from '@playwright/test';
import type { IntegrationStack } from '../tests/integration/support';

async function pair(page: Page, browser: Browser, stack: IntegrationStack) {
  const context = await browser.newContext();
  const peer = await context.newPage();
  const a = await signup(page, stack);
  const b = await signup(peer, stack);
  await startWorker(stack);
  await expect(page.getByText('已连接', { exact: true })).toBeVisible();
  await expect(peer.getByText('已连接', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: '发起聊天', exact: true }).click();
  await page.getByLabel('对方的用户名', { exact: true }).fill(b.username);
  await page.getByRole('button', { name: '查找用户', exact: true }).click();
  await expect(
    page.getByRole('button', { name: '开始聊天', exact: true }),
  ).toBeVisible();
  const avatarBounds = await page
    .getByRole('dialog')
    .locator('[data-slot="avatar"]')
    .boundingBox();
  expect(avatarBounds).not.toBeNull();
  expect(avatarBounds!.width).toBeCloseTo(avatarBounds!.height, 0);

  await page.getByRole('button', { name: '开始聊天', exact: true }).click();
  await expect(page).toHaveURL(/\/app\/chats\//);
  const link = peer
    .getByRole('navigation', { name: '会话列表' })
    .getByRole('link')
    .filter({ hasText: a.username });
  await expect(link).toBeVisible();
  await link.click();
  await expect(peer).toHaveURL(/\/app\/chats\//);
  return { peer, context, a, b };
}

test('two users find each other, send messages and preserve keyboard/IME behavior', async ({
  page,
  browser,
  stack,
}) => {
  const { peer, context } = await pair(page, browser, stack);
  try {
    const composer = page.getByRole('textbox', { name: '输入消息' });
    await composer.fill('你好，来自真实浏览器');
    await composer.press('Enter');
    await expect(
      peer.getByRole('log').getByText('你好，来自真实浏览器', { exact: true }),
    ).toBeVisible();
    await composer.fill('第一行');
    await composer.press('Shift+Enter');
    await composer.press('a');
    await expect(composer).toHaveValue('第一行\na');
    await composer.dispatchEvent('compositionstart');
    await composer.dispatchEvent('keydown', {
      key: 'Enter',
      code: 'Enter',
      isComposing: true,
    });
    await expect(composer).toHaveValue('第一行\na');
    await composer.dispatchEvent('compositionend');
    await composer.press('Enter');
    await expect(
      peer.getByRole('log').getByText('第一行\na', { exact: true }),
    ).toBeVisible();
    await page.screenshot({
      path: test.info().outputPath('chat-desktop.png'),
    });
  } finally {
    await context.close();
  }
});

test('mobile navigation preserves the conversation and unsent draft', async ({
  page,
  browser,
  stack,
}) => {
  const { context } = await pair(page, browser, stack);
  try {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.screenshot({
      path: test.info().outputPath('chat-mobile.png'),
    });
    const route = page.url();
    await page.getByRole('textbox', { name: '输入消息' }).fill('还没说完的话');
    await page.getByRole('button', { name: '返回会话列表' }).click();
    await expect(
      page.getByRole('navigation', { name: '会话列表' }),
    ).toBeVisible();
    await page
      .getByRole('navigation', { name: '会话列表' })
      .getByRole('link')
      .first()
      .click();
    await expect(page).toHaveURL(route);
    await expect(page.getByRole('textbox', { name: '输入消息' })).toHaveValue(
      '还没说完的话',
    );
  } finally {
    await context.close();
  }
});

test('a hidden chat page does not mark a new message read', async ({
  page,
  browser,
  stack,
}) => {
  const { peer, context } = await pair(page, browser, stack);
  try {
    await page.evaluate(() => {
      Object.defineProperty(document, 'visibilityState', {
        configurable: true,
        get: () => 'hidden',
      });
      document.dispatchEvent(new Event('visibilitychange'));
    });
    await peer
      .getByRole('textbox', { name: '输入消息' })
      .fill('隐藏标签页中的未读消息');
    await peer.getByRole('textbox', { name: '输入消息' }).press('Enter');
    await expect(
      page
        .getByRole('log')
        .getByText('隐藏标签页中的未读消息', { exact: true }),
    ).toBeVisible();
    const cookie = (await page.context().cookies()).find(
      (c) => c.name === 'chat_session',
    )!;
    const id = page.url().split('/').at(-1)!;
    expect(
      (
        await (
          await api(
            stack,
            `${cookie.name}=${cookie.value}`,
            `/conversations/${id}`,
          )
        ).json()
      ).my_read_seq,
    ).toBe('0');
    await page.evaluate(() => {
      Reflect.deleteProperty(document, 'visibilityState');
      document.dispatchEvent(new Event('visibilitychange'));
    });
    await page.bringToFront();
    await expect
      .poll(
        async () =>
          (
            await (
              await api(
                stack,
                `${cookie.name}=${cookie.value}`,
                `/conversations/${id}`,
              )
            ).json()
          ).my_read_seq,
      )
      .toBe('2');
  } finally {
    await context.close();
  }
});

test('HTML is displayed as plain text and the local theme survives reload', async ({
  page,
  browser,
  stack,
}) => {
  const { peer, context } = await pair(page, browser, stack);
  try {
    const text = '<img src=x onerror=alert(1)>';
    await page.getByRole('textbox', { name: '输入消息' }).fill(text);
    await page.getByRole('textbox', { name: '输入消息' }).press('Enter');
    await expect(
      peer.getByRole('log').getByText(text, { exact: true }),
    ).toBeVisible();
    await expect(peer.getByRole('log').locator('img')).toHaveCount(0);
    await page.goto('/app/settings');
    await page.getByLabel('外观', { exact: true }).selectOption('dark');
    await page.reload();
    expect(
      await page.evaluate(() => document.documentElement.dataset.theme),
    ).toBe('dark');
  } finally {
    await context.close();
  }
});
