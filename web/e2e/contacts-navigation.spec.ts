import { test, expect, signup, sessionCookie } from './fixtures';
import { account, api, befriend } from '../tests/integration/support';

test('contacts share the workspace layout and returning to chat preserves the selected conversation and draft', async ({
  page,
  stack,
}) => {
  const peer = await account(stack);
  await signup(page, stack);
  const cookie = await sessionCookie(page);
  await befriend(stack, cookie, peer.cookie);
  await stack.startWorker();
  const conversation = await (
    await api(stack, cookie, '/conversations/direct', 'POST', {
      peer_id: peer.user.id,
    })
  ).json();
  await page.goto(`/app/chats/${conversation.id}`);
  await page.getByRole('textbox', { name: '输入消息' }).fill('保留这个草稿');

  const navigation = page.getByRole('navigation', { name: '主导航' });
  await expect(
    navigation.getByRole('link', { name: '聊天', exact: true }),
  ).toHaveAttribute('aria-current', 'page');
  const chatSidebar = await page
    .getByRole('complementary', { name: '会话列表' })
    .boundingBox();
  await navigation.getByRole('link', { name: '设置', exact: true }).click();
  await page
    .getByRole('navigation', { name: '主导航' })
    .getByRole('link', { name: '聊天', exact: true })
    .click();
  await expect(page).toHaveURL(new RegExp(`/app/chats/${conversation.id}$`));
  await navigation.getByRole('link', { name: '通讯录', exact: true }).click();
  await expect(
    navigation.getByRole('link', { name: '通讯录', exact: true }),
  ).toHaveAttribute('aria-current', 'page');
  const contactsSidebar = await page
    .getByRole('complementary', { name: '通讯录列表' })
    .boundingBox();
  expect(contactsSidebar?.x).toBe(chatSidebar?.x);
  expect(contactsSidebar?.width).toBe(chatSidebar?.width);
  await page.getByRole('list', { name: '好友' }).getByRole('button').click();
  await expect(page).toHaveURL(new RegExp(`person=${peer.user.id}`));
  await page.goBack();
  await expect(page).not.toHaveURL(/person=/);
  await page.goForward();
  const detail = page.getByRole('region', { name: '好友资料' });
  await expect(detail).toContainText(`@${peer.user.username}`);
  await page.screenshot({
    path: test.info().outputPath('contacts-desktop.png'),
    animations: 'disabled',
  });

  await navigation.getByRole('link', { name: '聊天', exact: true }).click();
  await expect(page).toHaveURL(new RegExp(`/app/chats/${conversation.id}$`));
  await expect(page.getByRole('textbox', { name: '输入消息' })).toHaveValue(
    '保留这个草稿',
  );
  await navigation.getByRole('link', { name: '通讯录', exact: true }).click();
  await expect(detail).toContainText(`@${peer.user.username}`);
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(detail).toBeVisible();
  await expect(
    page.getByRole('complementary', { name: '通讯录列表' }),
  ).toBeHidden();
  await page.getByRole('button', { name: '返回通讯录列表' }).click();
  await expect(
    page.getByRole('complementary', { name: '通讯录列表' }),
  ).toBeVisible();
  await expect(navigation).toBeVisible();
  await page.screenshot({
    path: test.info().outputPath('contacts-mobile.png'),
    animations: 'disabled',
  });
});

test('returning from another section restores the message reading position and draft', async ({
  page,
  stack,
}) => {
  const peer = await account(stack);
  await signup(page, stack);
  const cookie = await sessionCookie(page);
  await befriend(stack, cookie, peer.cookie);
  const conversation = await (
    await api(stack, cookie, '/conversations/direct', 'POST', {
      peer_id: peer.user.id,
    })
  ).json();
  for (let index = 0; index < 24; index++) {
    const response = await api(
      stack,
      peer.cookie,
      `/conversations/${conversation.id}/messages`,
      'POST',
      {
        client_message_id: crypto.randomUUID(),
        body: `历史消息 ${index}\n第二行用于形成可滚动的历史记录`,
        attachment_ids: [],
      },
    );
    expect(response.status).toBe(201);
  }
  await page.goto(`/app/chats/${conversation.id}`);
  const timeline = page.locator('.message-scroll');
  await expect(page.locator('[data-message-id]')).toHaveCount(24);
  await page.getByRole('textbox', { name: '输入消息' }).fill('返回后继续写');
  await timeline.evaluate((element) => {
    element.scrollTop = 200;
  });
  await expect(
    page.getByRole('button', { name: '查看最新消息' }),
  ).toBeVisible();
  const before = await timeline.evaluate((element) => element.scrollTop);
  await page
    .getByRole('navigation', { name: '主导航' })
    .getByRole('link', { name: '设置', exact: true })
    .click();
  await page
    .getByRole('navigation', { name: '主导航' })
    .getByRole('link', { name: '聊天', exact: true })
    .click();

  await expect(page.getByRole('textbox', { name: '输入消息' })).toHaveValue(
    '返回后继续写',
  );
  await expect
    .poll(() => timeline.evaluate((element) => element.scrollTop))
    .toBeCloseTo(before, 0);
  await expect(
    page.getByRole('button', { name: '查看最新消息' }),
  ).toBeVisible();
  await page.screenshot({
    path: test.info().outputPath('chat-workspace-desktop.png'),
  });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(
    page.getByRole('button', { name: '返回会话列表' }),
  ).toBeVisible();
  await page.screenshot({
    path: test.info().outputPath('chat-workspace-mobile.png'),
  });
});
