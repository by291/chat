import { randomBytes } from 'node:crypto';

import { test as base, type Page } from '@playwright/test';

import {
  startIntegrationStack,
  type IntegrationStack,
} from '../tests/integration/support';

export const test = base.extend<{ stack: IntegrationStack }>({
  stack: [
    async ({}, use) => {
      const stack = await startIntegrationStack();

      try {
        await stack.startWeb();
        await use(stack);
      } finally {
        await stack.cleanup();
      }
    },
    { timeout: 120000 },
  ],
});

export { expect } from '@playwright/test';

export async function signup(
  page: Page,
  stack: IntegrationStack,
  password = 'correct horse battery',
) {
  const username = `e${randomBytes(6).toString('hex')}`;

  await page.goto('/register');
  await page.getByLabel('用户名', { exact: true }).fill(username);
  await page.getByLabel('昵称', { exact: true }).fill('测试用户 🌟');
  await page.getByLabel('密码', { exact: true }).fill(password);
  await page.getByRole('button', { name: '创建账号', exact: true }).click();

  await base.expect(page).toHaveURL(/\/app/);

  return { username, password };
}

export async function sessionCookie(page: Page) {
  const session = (await page.context().cookies()).find(
    (cookie) => cookie.name === 'chat_session',
  );
  if (!session) throw new Error('Expected authenticated browser');
  return `${session.name}=${session.value}`;
}

/** Follow the contact-to-conversation workflow instead of bypassing its selection. */
export async function startChatWith(page: Page, username: string) {
  await page
    .getByRole('navigation', { name: '主导航' })
    .getByRole('link', { name: /^通讯录/ })
    .click();
  await page.getByRole('button', { name: '好友', exact: true }).click();
  await page.getByRole('textbox', { name: '搜索通讯录' }).fill(username);
  const row = page
    .getByRole('list', { name: '好友', exact: true })
    .getByRole('button')
    .filter({ hasText: '@' + username });
  await row.click();
  await page.getByRole('button', { name: '发消息', exact: true }).click();
  await base.expect(page).toHaveURL(/\/app\/chats\//);
}

export async function editOwnProfile(page: Page) {
  await page.getByRole('button', { name: '我的账号', exact: true }).click();
}
