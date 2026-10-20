import { test, expect, signup, sessionCookie } from './fixtures';
import { account, api, befriend } from '../tests/integration/support';

test('own profile edits in a centered dialog while chat info stays local and preserves drafts', async ({
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
  await page.goto(`/app/chats/${conversation.id}`);
  const composer = page.getByRole('textbox', { name: '输入消息' });
  await composer.fill('修改资料后继续写');
  const destination = page.url();
  await page.getByRole('button', { name: '我的账号', exact: true }).click();
  await page.getByRole('menuitem', { name: '个人资料', exact: true }).click();
  const profile = page.getByRole('dialog', { name: '个人资料', exact: true });

  await expect(profile).toBeVisible();
  await expect(page).toHaveURL(destination);
  await expect(profile.getByLabel('昵称', { exact: true })).toHaveCount(0);
  const bounds = await profile.boundingBox();
  expect(
    Math.abs(bounds!.x + bounds!.width / 2 - page.viewportSize()!.width / 2),
  ).toBeLessThan(3);
  await page.screenshot({
    path: test.info().outputPath('profile-desktop.png'),
  });
  await profile.getByRole('button', { name: '编辑资料', exact: true }).click();
  await page.getByLabel('昵称', { exact: true }).fill('新的昵称');
  await page.getByRole('button', { name: '保存资料', exact: true }).click();
  await expect(profile).toContainText('新的昵称');
  await expect(page.getByLabel('昵称', { exact: true })).toHaveCount(0);
  await page.keyboard.press('Escape');
  await expect(composer).toHaveValue('修改资料后继续写');
  await expect(
    page.getByRole('button', { name: '我的账号', exact: true }),
  ).toBeFocused();

  await page.getByRole('button', { name: '聊天资料', exact: true }).click();
  await expect(
    page.getByRole('dialog', { name: '聊天资料', exact: true }),
  ).toContainText(peer.user.username);
  await page.screenshot({
    path: test.info().outputPath('peer-panel-desktop.png'),
  });
  await expect(page).toHaveURL(destination);
  await page.keyboard.press('Escape');
  await expect(composer).toHaveValue('修改资料后继续写');
  await page.goto('/app/settings');
  await expect(
    page
      .getByRole('navigation', { name: '设置分类' })
      .getByRole('link', { name: '个人资料' }),
  ).toHaveCount(0);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByRole('button', { name: '我的账号', exact: true }).click();
  await page.getByRole('menuitem', { name: '个人资料', exact: true }).click();
  await expect(profile).toBeVisible();
  await page.screenshot({ path: test.info().outputPath('profile-mobile.png') });
  await page.keyboard.press('Escape');
  await page.goto(destination);
  await page.getByRole('button', { name: '聊天资料', exact: true }).click();
  const peerPanel = page.getByRole('dialog', { name: '聊天资料', exact: true });
  await expect(peerPanel).toBeVisible();
  await page.screenshot({
    path: test.info().outputPath('peer-panel-mobile.png'),
  });
});

test('friend requests keep the search field and person rows across categories', async ({
  page,
  stack,
}) => {
  const sender = await account(stack);
  await signup(page, stack);
  const cookie = await sessionCookie(page);
  const me = await (await api(stack, cookie, '/auth/me')).json();
  expect(
    (
      await api(stack, sender.cookie, '/friend-requests', 'POST', {
        user_id: me.id,
      })
    ).status,
  ).toBe(200);
  await page.goto('/app/contacts');
  const search = page.getByRole('textbox', { name: '搜索通讯录' });
  await expect(search).toBeVisible();
  const before = await search.boundingBox();
  await page.getByRole('button', { name: /^新的朋友/ }).click();
  await expect(search).toBeVisible();
  expect(await search.boundingBox()).toEqual(before);
  const row = page.getByRole('list', { name: '新的朋友' }).getByRole('button');
  await expect(row).toContainText(sender.user.username);
  await row.click();
  await expect(page.getByRole('region', { name: '好友申请' })).toContainText(
    sender.user.username,
  );
  await page.screenshot({
    path: test.info().outputPath('request-desktop.png'),
  });
  await expect(
    page.getByRole('button', { name: '发起聊天', exact: true }),
  ).toHaveCount(0);
  await page.getByRole('button', { name: '申请筛选', exact: true }).click();
  await page.getByRole('menuitem', { name: '发出的申请', exact: true }).click();
  await expect(
    page.getByRole('button', { name: '已发送', exact: true }),
  ).toBeVisible();
});

test('leaving the chat page aborts a pending group join without redirecting back', async ({
  page,
  stack,
}) => {
  const owner = await account(stack);
  const group = await (
    await api(stack, owner.cookie, '/conversations/groups', 'POST', {
      title: 'Deferred join',
    })
  ).json();
  const invitation = await (
    await api(
      stack,
      owner.cookie,
      `/conversations/${group.id}/invitations`,
      'POST',
      {
        membership_id: group.membership.membership_id,
      },
    )
  ).json();
  await signup(page, stack);
  await page.goto('/app/settings');
  await page
    .getByRole('navigation', { name: '主导航' })
    .getByRole('link', { name: '聊天', exact: true })
    .click();
  let started!: () => void;
  const pending = new Promise<void>((resolve) => (started = resolve));
  let release!: () => void;
  const released = new Promise<void>((resolve) => (release = resolve));
  let aborted = false;
  page.on('requestfailed', (request) => {
    if (request.url().endsWith('/group-invitations/accept')) aborted = true;
  });
  await page.route('**/api/v1/group-invitations/accept', async (route) => {
    const response = await route.fetch();
    started();
    await released;
    // Client cancellation after commit deliberately makes the response undeliverable.
    await route.fulfill({ response }).catch(() => {});
  });
  try {
    await page.getByRole('button', { name: '群聊操作', exact: true }).click();
    await page.getByRole('menuitem', { name: '加入群聊', exact: true }).click();
    await page
      .getByLabel('邀请链接或口令', { exact: true })
      .fill(invitation.token);
    await page.getByRole('button', { name: '加入群聊', exact: true }).click();
    await pending;
    await page.goBack();

    await expect.poll(() => aborted).toBe(true);
    release();
    await expect(page).toHaveURL(/\/app\/settings$/);
  } finally {
    release();
  }
});
