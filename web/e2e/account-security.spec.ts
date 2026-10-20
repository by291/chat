import { test, expect, signup } from './fixtures';
import { api, eventually, sdk } from '../tests/integration/support';

test('a delayed password-change response preserves a newer shared-browser login', async ({
  page,
  context,
  stack,
}) => {
  const credentials = await signup(page, stack);
  const other = await context.newPage();
  let release!: () => void;
  let committed!: () => void;
  const held = new Promise<void>((resolve) => (release = resolve));
  const changed = new Promise<void>((resolve) => (committed = resolve));
  await page.route('**/api/v1/auth/password', async (route) => {
    const response = await route.fetch();
    expect(response.status()).toBe(204);
    committed();
    await held;
    await route.fulfill({ response });
  });

  try {
    await page.goto('/app/settings#settings-security');
    await page
      .getByLabel('当前密码', { exact: true })
      .fill(credentials.password);
    await page
      .getByLabel('新密码', { exact: true })
      .fill('new shared password');
    await page
      .getByLabel('确认新密码', { exact: true })
      .fill('new shared password');
    await page.getByRole('button', { name: '修改密码', exact: true }).click();
    await changed;
    await other.goto('/login');
    await other
      .getByLabel('用户名', { exact: true })
      .fill(credentials.username);
    await other.getByLabel('密码', { exact: true }).fill('new shared password');
    await other.getByRole('button', { name: '登录', exact: true }).click();
    await expect(other).toHaveURL(/\/app/);
    const fresh = (await context.cookies()).find(
      (cookie) => cookie.name === 'chat_session',
    )!;
    expect(fresh).toBeDefined();

    const acknowledged = page.waitForResponse('**/api/v1/auth/password');
    release();
    await acknowledged;
    // The login route restores the newer shared cookie immediately. Assert its
    // stable destination rather than racing that intermediate redirect.
    await expect(page).toHaveURL(/\/app$/);
    await expect(page.getByLabel('当前密码', { exact: true })).toHaveCount(0);

    expect(
      (await context.cookies()).find((cookie) => cookie.name === 'chat_session')
        ?.value,
    ).toBe(fresh.value);
    expect((await other.request.get('/api/v1/auth/me')).status()).toBe(200);
    await page.goto('/app/settings#settings-security');
    await expect(page.getByLabel('当前密码', { exact: true })).toHaveValue('');
    await other.reload();
    await expect(other).toHaveURL(/\/app/);
  } finally {
    release();
    await other.close();
  }
});

test('changing a password signs out all devices while an incorrect password preserves login', async ({
  page,
  browser,
  stack,
}) => {
  const credentials = await signup(page, stack);
  const other = await browser.newPage({ baseURL: 'http://localhost:5174' });
  let socket: ReturnType<typeof sdk> | undefined;
  try {
    await other.goto('/login');
    await other
      .getByLabel('用户名', { exact: true })
      .fill(credentials.username);
    await other.getByLabel('密码', { exact: true }).fill(credentials.password);
    await other.getByRole('button', { name: '登录', exact: true }).click();
    await expect(other).toHaveURL(/\/app/);
    const cookie = (await other.context().cookies()).find(
      (entry) => entry.name === 'chat_session',
    )!;
    const otherCookie = `${cookie.name}=${cookie.value}`;
    socket = sdk(stack, otherCookie);
    const disconnections: number[] = [];
    socket.client.on('disconnected', (event) =>
      disconnections.push(event.code),
    );
    await socket.connect();
    await stack.startWorker();
    await page.goto('/app/settings#settings-security');
    await page
      .getByLabel('当前密码', { exact: true })
      .fill('incorrect password');
    await page
      .getByLabel('新密码', { exact: true })
      .fill('new correct password');
    await page
      .getByLabel('确认新密码', { exact: true })
      .fill('new correct password');

    await page.getByRole('button', { name: '修改密码', exact: true }).click();

    await expect(page.getByRole('alert')).toHaveText('当前密码不正确');
    await expect(page).toHaveURL(/\/app\/settings/);
    expect((await api(stack, otherCookie, '/auth/me')).status).toBe(200);
    await page
      .getByLabel('当前密码', { exact: true })
      .fill(credentials.password);
    await page.getByRole('button', { name: '修改密码', exact: true }).click();

    await expect(page).toHaveURL(/\/login/);
    expect((await api(stack, otherCookie, '/auth/me')).status).toBe(401);
    await eventually(async () => disconnections.includes(4501));
    await other.reload();
    await expect(other).toHaveURL(/\/login/);
    await page.getByLabel('用户名', { exact: true }).fill(credentials.username);
    await page.getByLabel('密码', { exact: true }).fill(credentials.password);
    await page.getByRole('button', { name: '登录', exact: true }).click();
    await expect(page.getByRole('alert')).toBeVisible();
    await expect(page).toHaveURL(/\/login/);
    await page.getByLabel('密码', { exact: true }).fill('new correct password');
    await page.getByRole('button', { name: '登录', exact: true }).click();
    await expect(page).toHaveURL(/\/app/);
  } finally {
    socket?.stop();
    await other.close();
  }
});
