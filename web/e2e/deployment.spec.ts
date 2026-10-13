import { test, expect } from '@playwright/test';
import { fileURLToPath } from 'node:url';
import { randomBytes } from 'node:crypto';

const appOrigin = new URL(process.env.CHAT_DEPLOYMENT_ORIGIN!).origin;
const mediaOrigin = new URL(process.env.CHAT_DEPLOYMENT_MEDIA_ORIGIN!).origin;
const bucket = process.env.CHAT_DEPLOYMENT_BUCKET ?? 'chat-media';

test('HTTPS serves SPA deep links and keeps internal endpoints private', async ({
  request,
}) => {
  expect((await request.get('/health/ready')).status()).toBe(200);
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
  ]) {
    expect((await request.get(path)).status()).toBe(404);
  }
  const response = await request.get('/api/v1/auth/me');
  expect(response.status()).toBe(401);
  expect(response.headers()['x-request-id']).toBe(
    (await response.json()).request_id,
  );

  for (const path of [
    '/',
    `/${bucket}`,
    `/${bucket}?list-type=2`,
    '/metrics',
    '/api/publish',
    `/${bucket}/objects/unsigned`,
  ]) {
    expect((await request.get(`${mediaOrigin}${path}`)).status()).toBe(404);
  }
  const preflight = await request.fetch(
    `${mediaOrigin}/${bucket}/staging/probe`,
    {
      method: 'OPTIONS',
      headers: { Origin: appOrigin, 'Access-Control-Request-Method': 'PUT' },
    },
  );
  expect(preflight.status()).toBe(204);
  expect(preflight.headers()['access-control-allow-origin']).toBe(appOrigin);
  expect(
    preflight.headers()['access-control-allow-credentials'],
  ).toBeUndefined();
  expect(
    (
      await request.fetch(`${mediaOrigin}/${bucket}/staging/probe`, {
        method: 'OPTIONS',
        headers: { Origin: 'https://untrusted.example' },
      })
    ).status(),
  ).toBe(404);
});

test('six-character accounts and real browser conversation work behind HTTPS', async ({
  page,
  browser,
}) => {
  test.setTimeout(60000);
  const invitations = [
    process.env.CHAT_DEPLOYMENT_INVITE_1,
    process.env.CHAT_DEPLOYMENT_INVITE_2,
  ];
  if (invitations.some((token) => !token)) {
    throw new Error(
      'Provide two fresh staging invitation tokens; deployment tests do not access a database',
    );
  }
  const other = await browser.newContext({
    ignoreHTTPSErrors: process.env.CHAT_DEPLOYMENT_INSECURE_TLS === 'true',
    baseURL: appOrigin,
  });
  const peer = await other.newPage();

  async function signup(target: typeof page, invitation: string) {
    const username = `p${randomBytes(5).toString('hex')}`;
    await target.goto(`/register?invite=${encodeURIComponent(invitation)}`);
    await target.getByLabel('用户名', { exact: true }).fill(username);
    await target.getByLabel('昵称', { exact: true }).fill('HTTPS 用户');
    await target.getByLabel('密码', { exact: true }).fill('abc123');
    await target.getByRole('button', { name: '创建账号', exact: true }).click();
    await expect(target).toHaveURL(/\/app/);
    return username;
  }

  try {
    await signup(page, invitations[0]!);
    const username = await signup(peer, invitations[1]!);
    await expect(page.getByText('已连接', { exact: true })).toBeVisible();
    await expect(peer.getByText('已连接', { exact: true })).toBeVisible();
    const cookie = (await page.context().cookies()).find(
      (entry) => entry.name === '__Host-chat_session',
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

    const grantResponse = page.waitForResponse(
      (response) =>
        response.url().endsWith('/media/uploads') &&
        response.request().method() === 'POST',
    );
    const sample = fileURLToPath(
      new URL('../tests/fixtures/sample.png', import.meta.url),
    );
    await page.getByLabel('选择附件', { exact: true }).setInputFiles(sample);
    const grant = await (await grantResponse).json();
    expect(new URL(grant.url).origin).toBe(mediaOrigin);
    await expect(page.getByText('已就绪', { exact: true })).toBeVisible({
      timeout: 15000,
    });
    await page.getByRole('button', { name: '发送消息', exact: true }).click();
    const picture = peer
      .getByRole('log')
      .getByRole('img', { name: 'sample.png', exact: true });
    await expect(picture).toBeVisible();
    await expect
      .poll(() =>
        picture.evaluate((image) => (image as HTMLImageElement).naturalWidth),
      )
      .toBe(512);

    const mediaGrant = await page
      .context()
      .request.get(
        `/api/v1/media/${grant.attachment.id}/download?variant=thumbnail`,
      );
    expect(mediaGrant.status()).toBe(200);
    const signed = await mediaGrant.json();
    const object = await page
      .context()
      .request.get(signed.url, { headers: { Origin: appOrigin } });
    expect(object.status()).toBe(200);
    expect(object.headers()['x-content-type-options']).toBe('nosniff');
    expect(object.headers()['cache-control']).toBe('private, no-store');
    expect(object.headers()['access-control-allow-origin']).toBe(appOrigin);
    expect((await page.context().request.delete(signed.url)).status()).toBe(
      404,
    );
    expect(
      (
        await page.context().request.get(signed.url, {
          headers: { Origin: 'https://untrusted.example' },
        })
      ).status(),
    ).toBe(404);

    const policy = await page.evaluate(() =>
      document
        .querySelector('meta[http-equiv="content-security-policy"]')
        ?.getAttribute('content'),
    );
    expect(policy).toContain(mediaOrigin);
    expect(policy).toContain('blob:');
    await page.goto('/app/settings');
    await page.getByLabel('选择头像', { exact: true }).setInputFiles(sample);
    await expect(page.getByText('已就绪', { exact: true })).toBeVisible({
      timeout: 15000,
    });
    await page.getByRole('button', { name: '保存头像', exact: true }).click();
    await expect(
      page.getByRole('img', { name: 'HTTPS 用户的头像', exact: true }),
    ).toBeVisible();
  } finally {
    await other.close();
  }
});
