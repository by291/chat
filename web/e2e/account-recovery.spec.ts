import { test as base, expect } from '@playwright/test';

import { TestSmtpServer } from '../tests/integration/mail-support';
import { TestRuntime } from '../tests/integration/runtime';
import { api, eventually, sdk } from '../tests/integration/support';

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

test('captured recovery mail resets once, ignores scanner visits and disconnects other devices', async ({
  page,
  browser,
  stack,
  mail,
}) => {
  await page.goto('/register');
  await page.getByLabel('用户名', { exact: true }).fill('recoveruser');
  await page.getByLabel('昵称', { exact: true }).fill('找回用户');
  await page.getByLabel('密码', { exact: true }).fill('correct horse battery');
  await page.getByRole('button', { name: '创建账号', exact: true }).click();
  await expect(page).toHaveURL(/\/app/);
  await page.goto('/app/settings');
  await page
    .getByRole('textbox', { name: '恢复邮箱', exact: true })
    .fill('recover@example.test');
  await page
    .getByLabel('验证当前密码', { exact: true })
    .fill('correct horse battery');
  await page.getByRole('button', { name: '发送验证邮件', exact: true }).click();
  await eventually(async () => mail.messages().length === 1);
  const verification = new URL(mail.messages()[0].link!);
  await page.goto(verification.pathname + verification.hash);
  await page.getByRole('button', { name: '确认验证邮箱', exact: true }).click();
  await expect(
    page.getByRole('heading', { name: '邮箱已验证', exact: true }),
  ).toBeVisible();
  const cookie = (await page.context().cookies()).find(
    (entry) => entry.name === 'chat_session',
  )!;
  const oldCookie = `${cookie.name}=${cookie.value}`;
  const socket = sdk(stack, oldCookie);
  const disconnections: number[] = [];
  socket.client.on('disconnected', (event) => disconnections.push(event.code));
  await socket.connect();
  const recoveryPage = await browser.newPage({
    baseURL: 'http://localhost:5174',
  });
  try {
    await recoveryPage.goto('/login');
    await recoveryPage
      .getByRole('link', { name: '忘记密码？', exact: true })
      .click();
    await recoveryPage
      .getByLabel('已验证邮箱', { exact: true })
      .fill('recover@example.test');
    await recoveryPage
      .getByRole('button', { name: '申请找回邮件', exact: true })
      .click();
    await expect(recoveryPage.getByRole('status')).toContainText(
      '如果此邮箱已验证',
    );
    await eventually(async () => mail.messages().length === 2);
    const reset = new URL(mail.messages()[1].link!);
    expect(
      (await recoveryPage.request.get(reset.origin + reset.pathname)).status(),
    ).toBe(200);
    expect(
      (
        await stack.db.query(
          "SELECT count(*)::integer AS count FROM account_security_challenges WHERE purpose='password_reset' AND consumed_at IS NOT NULL",
        )
      ).rows[0].count,
    ).toBe(0);

    await recoveryPage.goto(reset.pathname + reset.hash);
    await recoveryPage
      .getByLabel('新密码', { exact: true })
      .fill('new correct password');
    await recoveryPage
      .getByLabel('确认新密码', { exact: true })
      .fill('new correct password');
    await recoveryPage
      .getByRole('button', { name: '重置密码', exact: true })
      .click();

    await expect(
      recoveryPage.getByRole('heading', { name: '密码已重置', exact: true }),
    ).toBeVisible();
    expect(new URL(recoveryPage.url()).hash).toBe('');
    expect((await api(stack, oldCookie, '/auth/me')).status).toBe(401);
    await eventually(async () => disconnections.includes(4501));
    expect(
      (
        await recoveryPage.request.get(`${stack.apiUrl}/api/v1/auth/me`)
      ).status(),
    ).toBe(401);
    expect(
      await recoveryPage.evaluate(() =>
        JSON.stringify({ ...localStorage, ...sessionStorage }),
      ),
    ).not.toContain(reset.hash.slice(1));
    await recoveryPage
      .getByRole('link', { name: '使用新密码登录', exact: true })
      .click();
    await recoveryPage
      .getByLabel('用户名', { exact: true })
      .fill('recoveruser');
    await recoveryPage
      .getByLabel('密码', { exact: true })
      .fill('correct horse battery');
    await recoveryPage
      .getByRole('button', { name: '登录', exact: true })
      .click();
    await expect(recoveryPage.getByRole('alert')).toBeVisible();
    await recoveryPage
      .getByLabel('密码', { exact: true })
      .fill('new correct password');
    await recoveryPage
      .getByRole('button', { name: '登录', exact: true })
      .click();
    await expect(recoveryPage).toHaveURL(/\/app/);
  } finally {
    socket.stop();
    await recoveryPage.close();
  }
});
