import { startChatWith } from './fixtures';
import { befriend } from '../tests/integration/support';
import { sessionCookie } from './fixtures';
import { test, expect, signup } from './fixtures';
import { account } from '../tests/integration/support';

test('connection notice shares one top position and disappears after recovery', async ({
  page,
  stack,
}) => {
  const partner = await account(stack);
  await signup(page, stack);
  await befriend(stack, await sessionCookie(page), partner.cookie);
  const shell = page.locator('.chat-shell');
  const notice = page.getByRole('status', { name: '连接状态' });
  await expect(shell).toHaveAttribute('data-connection', 'online');
  await expect(page.getByText('已连接', { exact: true })).toHaveCount(0);

  await startChatWith(page, partner.user.username);
  await expect(page).toHaveURL(/\/app\/chats\//);
  const conversationUrl = page.url();

  for (const width of [1280, 390, 320]) {
    await page.setViewportSize({ width, height: 900 });
    for (const route of ['/app', conversationUrl]) {
      await page.goto(route);
      await expect(shell).toHaveAttribute('data-connection', 'online');
      const columns = page.locator('.chat-columns');
      const normalTop = (await columns.boundingBox())!.y;
      expect((await notice.boundingBox())!.height).toBe(0);

      await page.context().setOffline(true);
      await expect(notice).toContainText('连接中断，正在重连，消息可能延迟');
      await expect(notice).toHaveCount(1);
      const noticeBounds = (await notice.boundingBox())!;
      const shellBounds = (await shell.boundingBox())!;
      const columnsBounds = (await columns.boundingBox())!;
      expect(noticeBounds.x).toBeCloseTo(shellBounds.x);
      expect(noticeBounds.y).toBeCloseTo(shellBounds.y);
      expect(noticeBounds.width).toBeCloseTo(shellBounds.width);
      expect(columnsBounds.y).toBeCloseTo(noticeBounds.y + noticeBounds.height);
      if (route === conversationUrl) {
        await expect(
          page.getByRole('textbox', { name: '输入消息' }),
        ).toBeInViewport();
        await page.screenshot({
          path: test.info().outputPath(`connection-offline-${width}.png`),
        });
      }

      await page.context().setOffline(false);
      await expect(shell).toHaveAttribute('data-connection', 'online');
      await expect(notice).toBeEmpty();
      expect((await columns.boundingBox())!.y).toBeCloseTo(normalTop);
      if (route === conversationUrl) {
        await page.screenshot({
          path: test.info().outputPath(`connection-online-${width}.png`),
        });
      }
    }
  }
});
