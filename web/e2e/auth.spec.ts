import { test, expect, signup } from './fixtures';
import { api } from '../tests/integration/support';
test('six-character password registers, logs in and restores identity', async ({
  page,
  stack,
}) => {
  const credentials = await signup(page, stack, 'abc123');
  await page.reload();
  await expect(
    page.getByRole('button', { name: '退出登录', exact: true }),
  ).toBeVisible();
  expect(
    await page.evaluate(() => localStorage.getItem('session_token')),
  ).toBeNull();
  expect(await page.evaluate(() => document.cookie)).not.toContain(
    'chat_session',
  );
  await page.goto('/app/settings');
  await expect(page.getByRole('heading', { name: '登录设备' })).toBeVisible();
  await expect(page.getByText('当前设备', { exact: true })).toBeVisible();
  expect(await page.locator('body').innerText()).not.toContain('token_hash');

  await page.getByRole('button', { name: '退出登录', exact: true }).click();
  await expect(page).toHaveURL(/\/login/);
  await page.getByLabel('用户名', { exact: true }).fill(credentials.username);
  await page.getByLabel('密码', { exact: true }).fill(credentials.password);
  await page.getByRole('button', { name: '登录', exact: true }).click();

  await expect(page).toHaveURL(/\/app/);
  await expect(
    page.getByRole('button', { name: '退出登录', exact: true }),
  ).toBeVisible();
});
test('revoked device returns to login', async ({ page, stack }) => {
  const credentials = await signup(page, stack);
  const cookie = (await page.context().cookies()).find(
    (c) => c.name === 'chat_session',
  )!;
  const own = await (
    await api(stack, `${cookie.name}=${cookie.value}`, '/sessions')
  ).json();
  const response = await fetch(`${stack.apiUrl}/api/v1/auth/login`, {
    method: 'POST',
    headers: { Origin: stack.origin, 'Content-Type': 'application/json' },
    body: JSON.stringify({ ...credentials, device_label: 'Other device' }),
  });
  const other = response.headers.get('set-cookie')!.split(';')[0];
  expect(
    (
      await api(
        stack,
        other,
        `/sessions/${own.find((s: { is_current: boolean }) => s.is_current).id}`,
        'DELETE',
      )
    ).status,
  ).toBe(204);
  await page.reload();
  await expect(page).toHaveURL(/\/login/);
});
test('profile keeps Unicode and unauthenticated app visits redirect', async ({
  page,
  stack,
}) => {
  await page.goto('/app');
  await expect(page).toHaveURL(/\/login/);
  await signup(page, stack);
  await page.goto('/app/settings');
  await page.getByLabel('昵称', { exact: true }).fill('中文昵称 🪁');
  await page.getByRole('button', { name: '保存资料', exact: true }).click();
  await expect(page.getByText('资料已保存', { exact: true })).toBeVisible();
  await page.reload();
  await expect(page.getByLabel('昵称', { exact: true })).toHaveValue(
    '中文昵称 🪁',
  );
});
