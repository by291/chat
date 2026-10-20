import { editOwnProfile } from './fixtures';
import { befriend } from '../tests/integration/support';
import { test as base, expect } from '@playwright/test';
import { fileURLToPath } from 'node:url';
import { readFile } from 'node:fs/promises';

import { TestRuntime } from '../tests/integration/runtime';
import { signup } from './fixtures';
import { account, api } from '../tests/integration/support';

const test = base.extend<{ stack: TestRuntime }>({
  stack: [
    async ({}, use) => {
      const stack = await TestRuntime.start();
      try {
        await stack.startWeb({ mode: 'preview' });
        await use(stack);
      } finally {
        await stack.cleanup();
      }
    },
    { timeout: 120000 },
  ],
});

test('a second production build waits for consent while a draft and signed upload stay intact', async ({
  page,
  context,
  stack,
}) => {
  test.setTimeout(120000);
  await signup(page, stack);
  await expect
    .poll(() =>
      page.evaluate(
        async () =>
          (await navigator.serviceWorker.getRegistration())?.active?.state,
      ),
    )
    .toBe('activated');
  await page.reload();
  await stack.startWorker();
  const peer = await account(stack);
  const cookie = (await context.cookies()).find(
    (entry) => entry.name === 'chat_session',
  )!;
  await befriend(stack, `${cookie.name}=${cookie.value}`, peer.cookie);
  const conversation = await (
    await api(
      stack,
      `${cookie.name}=${cookie.value}`,
      '/conversations/direct',
      'POST',
      { peer_id: peer.user.id },
    )
  ).json();
  await page.goto(`/app/chats/${conversation.id}`);
  await page
    .getByRole('textbox', { name: '输入消息' })
    .fill('更新前保留的草稿');
  let release!: () => void;
  const blocked = new Promise<void>((resolve) => {
    release = resolve;
  });
  await context.route(
    new URL(stack.env.MEDIA_S3_PUBLIC_ENDPOINT).origin + '/**',
    async (route) => {
      if (route.request().method() === 'PUT') await blocked;
      await route.continue();
    },
  );
  try {
    await page.getByLabel('选择附件', { exact: true }).setInputFiles({
      name: 'pending-update.png',
      mimeType: 'image/png',
      buffer: await readFile(
        new URL('../tests/fixtures/sample.png', import.meta.url),
      ),
    });
    await expect(
      page.getByText('pending-update.png', { exact: true }),
    ).toBeVisible();
    await page.evaluate(async () => {
      await caches.open('another-application');
    });

    await stack.stopWeb();
    await stack.startWeb({ mode: 'preview', version: 'two' });
    await page.evaluate(async () => {
      await (await navigator.serviceWorker.getRegistration())!.update();
    });
    await expect
      .poll(() =>
        page.evaluate(
          async () =>
            (await navigator.serviceWorker.getRegistration())?.waiting?.state,
        ),
      )
      .toBe('installed');

    await expect(page.getByRole('textbox', { name: '输入消息' })).toHaveValue(
      '更新前保留的草稿',
    );
    await expect(
      page.getByText('pending-update.png', { exact: true }),
    ).toBeVisible();
    await expect(
      page.getByRole('button', { name: '更新并重新加载', exact: true }),
    ).toBeVisible();
    await page
      .getByRole('button', { name: '更新并重新加载', exact: true })
      .click();
    await page
      .getByRole('button', { name: '继续当前版本', exact: true })
      .click();
    await expect(page.getByRole('textbox', { name: '输入消息' })).toHaveValue(
      '更新前保留的草稿',
    );
    await expect(
      page.getByText('pending-update.png', { exact: true }),
    ).toBeVisible();
    await page
      .getByRole('button', { name: '更新并重新加载', exact: true })
      .click();
    await page
      .getByRole('button', { name: '确认更新并重新加载', exact: true })
      .click();
    release();

    await expect(page.getByRole('textbox', { name: '输入消息' })).toHaveValue(
      '',
    );
    await expect
      .poll(() =>
        page.evaluate(async () =>
          (await caches.keys()).filter((name) =>
            name.startsWith('chat-pwa-assets:'),
          ),
        ),
      )
      .toEqual(['chat-pwa-assets:web-preview-two']);
    expect(
      await page.evaluate(async () =>
        (await caches.keys()).includes('another-application'),
      ),
    ).toBe(true);
  } finally {
    release();
  }
});

test('production worker caches public assets only and offline navigation shows generic content', async ({
  page,
  context,
  stack,
}) => {
  await signup(page, stack);
  await expect
    .poll(() =>
      page.evaluate(
        async () =>
          (await navigator.serviceWorker.getRegistration())?.active?.state,
      ),
    )
    .toBe('activated');
  await page.reload();
  await expect
    .poll(() => page.evaluate(() => !!navigator.serviceWorker.controller))
    .toBe(true);
  const manifest = await (
    await page.request.get('/manifest.webmanifest')
  ).json();
  expect(manifest.start_url).toBe('/app');
  expect(manifest.scope).toBe('/');
  expect(manifest.display).toBe('standalone');
  for (const icon of manifest.icons) {
    expect((await page.request.get(icon.src)).status()).toBe(200);
  }
  await page.goto('/app/settings');
  await expect(
    page.getByRole('heading', { name: '设置', exact: true }),
  ).toBeVisible();
  await stack.startWorker();
  await editOwnProfile(page);
  await page
    .getByLabel('选择头像', { exact: true })
    .setInputFiles(
      fileURLToPath(new URL('../tests/fixtures/sample.png', import.meta.url)),
    );
  await expect(page.getByText('已就绪', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: '保存头像', exact: true }).click();
  await expect(
    page.getByRole('img', { name: '测试用户 🌟的头像', exact: true }),
  ).toBeVisible();
  const keys = await page.evaluate(async () => {
    const urls: string[] = [];
    for (const name of await caches.keys()) {
      for (const request of await (await caches.open(name)).keys())
        urls.push(request.url);
    }
    return urls;
  });
  expect(
    keys.some((url) => new URL(url).pathname.startsWith('/_app/immutable/')),
  ).toBe(true);
  expect(
    keys.some((url) =>
      /\/api\/|\/media\/|signature=|token=|\/app(?:\/|$)/.test(url),
    ),
  ).toBe(false);
  await context.setOffline(true);
  try {
    const asset = keys.find((url) => url.endsWith('.js'))!;
    expect(
      await page.evaluate(async (url) => (await fetch(url)).status, asset),
    ).toBe(200);
    await page.goto('/app');
    await expect(
      page.getByRole('heading', { name: '暂时离线', exact: true }),
    ).toBeVisible();
    await expect(page.getByRole('textbox', { name: '输入消息' })).toHaveCount(
      0,
    );
  } finally {
    await context.setOffline(false);
  }
});
