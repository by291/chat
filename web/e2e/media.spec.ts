import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';

import { test, expect, signup } from './fixtures';
import { startWorker } from '../tests/integration/support';

const sample = fileURLToPath(
  new URL('../tests/fixtures/sample.png', import.meta.url),
);

test('real signed uploads report progress, retry and send image/files with avatar controls', async ({
  page,
  browser,
  stack,
}) => {
  test.setTimeout(60000);
  const context = await browser.newContext();
  const peer = await context.newPage();
  try {
    await signup(page, stack);
    const account = await signup(peer, stack);
    await startWorker(stack);
    await page.getByRole('button', { name: '发起聊天', exact: true }).click();
    await page
      .getByLabel('对方的用户名', { exact: true })
      .fill(account.username);
    await page.getByRole('button', { name: '查找用户', exact: true }).click();
    await page.getByRole('button', { name: '开始聊天', exact: true }).click();
    await expect(page).toHaveURL(/\/app\/chats\//);
    await peer.goto(new URL(page.url()).pathname);
    await page.evaluate(() => {
      const state = window as Window & { transferProgress?: number[] };
      state.transferProgress = [];
      const send = XMLHttpRequest.prototype.send;
      XMLHttpRequest.prototype.send = function (body) {
        this.upload.addEventListener('progress', (event) => {
          if (event.lengthComputable)
            state.transferProgress!.push(event.loaded);
        });
        return send.call(this, body);
      };
    });

    let interrupted = false;
    await page.route('http://localhost:8337/**', async (route) => {
      if (!interrupted && route.request().method() === 'PUT') {
        interrupted = true;
        await route.abort('internetdisconnected');
      } else await route.continue();
    });
    await page.getByLabel('选择附件', { exact: true }).setInputFiles(sample);
    await expect(
      page.getByRole('button', { name: '重试', exact: true }),
    ).toBeVisible();
    await page.getByRole('button', { name: '重试', exact: true }).click();
    await expect(page.getByText('已就绪', { exact: true })).toBeVisible();
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
    await expect
      .poll(() =>
        page.evaluate(
          () =>
            (window as Window & { transferProgress?: number[] })
              .transferProgress!.length,
        ),
      )
      .toBeGreaterThan(0);

    const bytes = Buffer.alloc(512 * 1024, 'g');
    await page.getByLabel('选择附件', { exact: true }).setInputFiles({
      name: 'notes.bin',
      mimeType: 'application/octet-stream',
      buffer: bytes,
    });
    await expect(page.getByText('已就绪', { exact: true })).toBeVisible();
    await page.getByRole('button', { name: '发送消息', exact: true }).click();
    const downloaded = peer.waitForEvent('download');
    await peer.getByRole('button', { name: /notes\.bin/ }).click();
    const download = await downloaded;
    expect(download.suggestedFilename()).toBe('notes.bin');
    const path = await download.path();
    expect(
      createHash('sha256')
        .update(await readFile(path!))
        .digest('hex'),
    ).toBe(createHash('sha256').update(bytes).digest('hex'));

    await page.goto('/app/settings');
    await page.getByLabel('选择头像', { exact: true }).setInputFiles(sample);
    await expect(page.getByText('已就绪', { exact: true })).toBeVisible();
    await page.getByRole('button', { name: '保存头像', exact: true }).click();
    await expect(
      page.getByRole('img', { name: '测试用户 🌟的头像', exact: true }),
    ).toBeVisible();
    await page.getByRole('button', { name: '移除头像', exact: true }).click();
    await expect(
      page.getByRole('img', { name: '测试用户 🌟的头像', exact: true }),
    ).toHaveCount(0);
    await expect(
      page
        .getByRole('region', { name: '头像', exact: true })
        .getByText('测', { exact: true }),
    ).toBeVisible();
    await peer.setViewportSize({ width: 390, height: 844 });
    await peer.screenshot({
      path: '../.superpowers/sdd/2026-10-13-seaweedfs-media/media-mobile.png',
    });
  } finally {
    await context.close();
  }
});
