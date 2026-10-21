import { account, api, befriend } from '../tests/integration/support';
import { test, expect, signup, sessionCookie } from './fixtures';

test('adaptive workspace keeps navigation and chat usable across screen sizes', async ({
  page,
  stack,
}) => {
  const peer = await account(stack);
  expect(
    (
      await api(stack, peer.cookie, '/users/me', 'PATCH', {
        display_name: '林小满',
      })
    ).status,
  ).toBe(200);
  await signup(page, stack);
  const cookie = await sessionCookie(page);
  await befriend(stack, cookie, peer.cookie);
  const conversation = await (
    await api(stack, cookie, '/conversations/direct', 'POST', {
      peer_id: peer.user.id,
    })
  ).json();
  for (const [sender, body] of [
    [peer.cookie, '周末一起出去走走吗？'],
    [cookie, '好呀，我正想找个地方放松一下。'],
    [peer.cookie, '那就去湖边吧。\n下午出发，走累了还能找家咖啡店坐坐。'],
    [cookie, '听起来不错，到时候见 ☕'],
  ]) {
    const response = await api(
      stack,
      sender,
      `/conversations/${conversation.id}/messages`,
      'POST',
      {
        client_message_id: crypto.randomUUID(),
        body,
        attachment_ids: [],
      },
    );
    expect(response.status).toBe(201);
  }
  const secondPeer = await account(stack);
  expect(
    (
      await api(stack, secondPeer.cookie, '/users/me', 'PATCH', {
        display_name: '陈一',
      })
    ).status,
  ).toBe(200);
  await befriend(stack, cookie, secondPeer.cookie);
  expect(
    (
      await api(stack, cookie, '/conversations/direct', 'POST', {
        peer_id: secondPeer.user.id,
      })
    ).status,
  ).toBe(201);
  await page.goto(`/app/chats/${conversation.id}`);

  await expect(
    page.getByRole('button', { name: '常聊的人', exact: true }),
  ).toHaveCount(0);
  await page.getByRole('textbox', { name: '输入消息' }).fill('草稿保留');
  const navigation = page.getByRole('navigation', { name: '主导航' });
  await navigation.getByRole('link', { name: '设置', exact: true }).click();
  await navigation.getByRole('link', { name: '聊天', exact: true }).click();
  await expect(page.getByRole('textbox', { name: '输入消息' })).toHaveValue(
    '草稿保留',
  );

  const overviewToggle = page.getByRole('button', {
    name: '聊天概览',
    exact: true,
  });
  await page.setViewportSize({ width: 1440, height: 900 });
  await expect(
    page.getByRole('complementary', { name: '聊天概览' }),
  ).toBeVisible();
  await page
    .getByRole('textbox', { name: '输入消息' })
    .fill('调整布局不丢草稿');
  await overviewToggle.click();
  await expect(
    page.getByRole('complementary', { name: '聊天概览' }),
  ).toBeHidden();
  await expect(page.getByRole('textbox', { name: '输入消息' })).toHaveValue(
    '调整布局不丢草稿',
  );
  await overviewToggle.click();
  await page.getByRole('textbox', { name: '输入消息' }).clear();

  for (const width of [1920, 1726, 1440, 1024, 768, 390, 320]) {
    await page.setViewportSize({ width, height: 900 });
    await expect(page.getByRole('textbox', { name: '输入消息' })).toBeVisible();
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= window.innerWidth,
      ),
    ).toBe(true);
    if (width >= 1440) {
      const sidebar = await page.locator('.conversation-sidebar').boundingBox();
      expect(sidebar!.width).toBeGreaterThanOrEqual(320);
      expect(sidebar!.width).toBeLessThanOrEqual(400);
      if (width === 1726) expect(sidebar!.width).toBeGreaterThan(360);
    }
    await page.screenshot({
      path: test.info().outputPath(`workspace-${width}.png`),
      animations: 'disabled',
    });
  }
  await page.setViewportSize({ width: 667, height: 375 });
  await page.getByRole('button', { name: '返回会话列表' }).click();
  const row = page
    .getByRole('navigation', { name: '会话列表' })
    .getByRole('link')
    .filter({ hasText: '林小满' });
  await row.scrollIntoViewIfNeeded();
  await expect(row).toBeInViewport({ ratio: 1 });
  expect(
    await row.evaluate((element) => {
      const bounds = element.getBoundingClientRect();
      return element.contains(
        document.elementFromPoint(
          bounds.x + bounds.width / 2,
          bounds.y + bounds.height / 2,
        ),
      );
    }),
  ).toBe(true);
  await page.screenshot({
    path: test.info().outputPath('workspace-short.png'),
    animations: 'disabled',
  });
  await row.click();
  await expect(page.getByRole('textbox', { name: '输入消息' })).toBeVisible();

  await page.setViewportSize({ width: 1440, height: 900 });
  await navigation.getByRole('link', { name: '设置', exact: true }).click();
  await page
    .getByRole('navigation', { name: '设置分类' })
    .getByRole('link', { name: '通知外观' })
    .click();
  await page.getByLabel('外观', { exact: true }).selectOption('light');
  await page.reload();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
  await navigation.getByRole('link', { name: '聊天', exact: true }).click();
  await page.goto(`/app/chats/${conversation.id}`);
  await expect(page.getByRole('textbox', { name: '输入消息' })).toBeVisible();
  await page.screenshot({
    path: test.info().outputPath('workspace-light.png'),
    animations: 'disabled',
  });
});
