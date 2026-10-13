import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { createHash, randomBytes } from 'node:crypto';
import pg from 'pg';
test.use({ baseURL: 'https://localhost:5443', ignoreHTTPSErrors: true });
test('HTTPS serves SPA deep links and keeps internal endpoints private', async ({
  request,
}) => {
  for (const path of [
    '/login',
    '/app/chats/01900000-0000-7000-8000-000000000001',
  ]) {
    const response = await request.get(path);
    expect(response.status()).toBe(200);
    expect(await response.text()).toContain('<html');
  }
  for (const path of [
    '/internal/centrifugo/connect',
    '/metrics',
    '/api/publish',
  ])
    expect((await request.get(path)).status()).toBe(404);
  const response = await request.get('/api/v1/auth/me');
  expect(response.status()).toBe(401);
  const body = await response.json();
  expect(response.headers()['x-request-id']).toBe(body.request_id);
});
test('six-character accounts and real browser conversation work behind HTTPS', async ({
  page,
  browser,
}) => {
  const env = await readFile(
    fileURLToPath(
      new URL(
        '../../.superpowers/sdd/2026-10-12-phase-1-private-chat/deployment/test.env',
        import.meta.url,
      ),
    ),
    'utf8',
  );
  const values = Object.fromEntries(
    env
      .split(/\r?\n/)
      .filter((l) => /^[A-Z_]+=/.test(l))
      .map((l) => {
        const at = l.indexOf('=');
        return [l.slice(0, at), l.slice(at + 1)];
      }),
  );
  const db = new pg.Client({
    host: '127.0.0.1',
    port: 55434,
    user: values.POSTGRES_USER,
    password: values.POSTGRES_PASSWORD,
    database: values.POSTGRES_DB,
  });
  await db.connect();
  const other = await browser.newContext({
    ignoreHTTPSErrors: true,
    baseURL: 'https://localhost:5443',
  });
  const peer = await other.newPage();
  async function signup(target: typeof page) {
    const token = randomBytes(32).toString('hex'),
      username = `p${randomBytes(5).toString('hex')}`;
    await db.query(
      "INSERT INTO invitations(id,token_hash,expires_at) VALUES(gen_random_uuid(),$1,now()+interval '7 days')",
      [createHash('sha256').update(token).digest('hex')],
    );
    await target.goto(`/register?invite=${token}`);
    await target.getByLabel('用户名', { exact: true }).fill(username);
    await target.getByLabel('昵称', { exact: true }).fill('HTTPS 用户');
    await target.getByLabel('密码', { exact: true }).fill('abc123');
    await target.getByRole('button', { name: '创建账号', exact: true }).click();
    await expect(target).toHaveURL(/\/app/);
    return username;
  }
  try {
    await signup(page);
    const username = await signup(peer);
    await expect(page.getByText('已连接', { exact: true })).toBeVisible();
    await expect(peer.getByText('已连接', { exact: true })).toBeVisible();
    const cookie = (await page.context().cookies()).find(
      (c) => c.name === '__Host-chat_session',
    )!;
    expect(cookie.httpOnly).toBe(true);
    expect(cookie.secure).toBe(true);
    expect(cookie.sameSite).toBe('Lax');
    expect(cookie.path).toBe('/');
    await page.getByRole('button', { name: '发起聊天', exact: true }).click();
    await page.getByLabel('对方的用户名', { exact: true }).fill(username);
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
    await page
      .getByRole('textbox', { name: '输入消息' })
      .fill('经过 HTTPS 的真实消息');
    await page.getByRole('textbox', { name: '输入消息' }).press('Enter');
    await expect(
      peer.getByRole('log').getByText('经过 HTTPS 的真实消息', { exact: true }),
    ).toBeVisible();
    await page.reload();
    await expect(
      page.getByRole('log').getByText('经过 HTTPS 的真实消息', { exact: true }),
    ).toBeVisible();
  } finally {
    await other.close();
    await db.end();
  }
});
