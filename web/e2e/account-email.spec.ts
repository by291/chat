import { test as base, expect } from '@playwright/test';

import { TestSmtpServer } from '../tests/integration/mail-support';
import { TestRuntime } from '../tests/integration/runtime';
import { eventually } from '../tests/integration/support';

const test = base.extend<{ mail: TestSmtpServer; stack: TestRuntime }>({
  mail: async ({}, use) => {
    const mail = new TestSmtpServer();
    await mail.start();
    try {
      await use(mail);
    } finally {
      await mail.stop();
    }
  },
  stack: [
    async ({ mail }, use) => {
      const stack = await TestRuntime.start({ mail: mail.environment() });
      try {
        await stack.startWeb();
        await stack.startWorker();
        await use(stack);
      } finally {
        await stack.cleanup();
      }
    },
    { timeout: 120000 },
  ],
});

test('captured verification link stays in the fragment through inline login and only explicit confirmation consumes it', async ({
  page,
  stack,
  mail,
}) => {
  await page.goto('/register');
  await page.getByLabel('用户名', { exact: true }).fill('emailuser');
  await page.getByLabel('昵称', { exact: true }).fill('邮箱用户');
  await page.getByLabel('密码', { exact: true }).fill('correct horse battery');
  await page.getByRole('button', { name: '创建账号', exact: true }).click();
  await expect(page).toHaveURL(/\/app/);
  await page.goto('/app/settings#settings-security');
  await page
    .getByRole('textbox', { name: '恢复邮箱', exact: true })
    .fill('owned@example.test');
  await page
    .getByLabel('验证当前密码', { exact: true })
    .fill('correct horse battery');

  await page.getByRole('button', { name: '发送验证邮件', exact: true }).click();

  await expect(
    page.getByRole('status').filter({ hasText: '验证邮件已申请' }),
  ).toBeVisible();
  await eventually(async () => mail.messages().length === 1);
  const link = mail.messages()[0].link!;
  const target = new URL(link);
  const path = target.pathname + target.hash;
  await page.getByRole('button', { name: '我的账号', exact: true }).click();
  await page.getByRole('menuitem', { name: '退出登录', exact: true }).click();
  await expect(page).toHaveURL(/\/login/);
  const scanner = await page.request.get(target.origin + target.pathname);
  expect(scanner.status()).toBe(200);
  expect(
    (
      await stack.db.query(
        'SELECT count(*)::integer AS count FROM account_security_challenges WHERE consumed_at IS NOT NULL',
      )
    ).rows[0].count,
  ).toBe(0);

  await page.goto(path);
  await page.getByLabel('用户名', { exact: true }).fill('emailuser');
  await page.getByLabel('密码', { exact: true }).fill('correct horse battery');
  await page.getByRole('button', { name: '登录', exact: true }).click();
  await expect(
    page.getByRole('button', { name: '确认验证邮箱', exact: true }),
  ).toBeVisible();
  expect(new URL(page.url()).pathname).toBe('/verify-email');
  expect(new URL(page.url()).search).toBe('');
  expect(new URL(page.url()).hash).toBe(target.hash);
  expect(
    (
      await stack.db.query(
        'SELECT count(*)::integer AS count FROM account_security_challenges WHERE consumed_at IS NOT NULL',
      )
    ).rows[0].count,
  ).toBe(0);
  expect(
    await page.evaluate(() =>
      JSON.stringify({ ...localStorage, ...sessionStorage }),
    ),
  ).not.toContain(target.hash.slice(1));
  expect(
    await page.locator('meta[name="referrer"]').getAttribute('content'),
  ).toBe('no-referrer');

  await page.getByRole('button', { name: '确认验证邮箱', exact: true }).click();

  await expect(
    page.getByRole('heading', { name: '邮箱已验证', exact: true }),
  ).toBeVisible();
  expect(new URL(page.url()).hash).toBe('');
  await page.getByRole('link', { name: '返回个人设置', exact: true }).click();
  await expect(
    page.getByText('owned@example.test', { exact: true }),
  ).toBeVisible();
});
